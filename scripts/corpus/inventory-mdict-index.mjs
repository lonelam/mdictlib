#!/usr/bin/env node

import { mkdir, rename, rm, writeFile } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import path from "node:path";
import process from "node:process";
import { fileURLToPath, pathToFileURL } from "node:url";

const DEFAULT_ROOT = "https://mdx.mdict.org/";
const DEFAULT_TIMEOUT_MS = 15_000;
const DEFAULT_MAX_PAGES = 5_000;
const DEFAULT_MAX_FILES = 20_000;
const DEFAULT_MAX_PAGE_BYTES = 4 * 1024 * 1024;
const DEFAULT_MAX_IN_FLIGHT_PAGE_BYTES = 64 * 1024 * 1024;
const DEFAULT_CONCURRENCY = 8;

function usage() {
  return `Inventory a Caddy-style MDict file index without downloading payloads.

Usage:
  node scripts/corpus/inventory-mdict-index.mjs --output <inventory.json> [options]

Options:
  --output, -o <path>       Output JSON path; use - for stdout (required)
  --root <url>              Index root (default: ${DEFAULT_ROOT})
  --timeout-ms <number>     Per-page response timeout (default: ${DEFAULT_TIMEOUT_MS})
  --max-pages <number>      Maximum directory pages (default: ${DEFAULT_MAX_PAGES})
  --max-files <number>      Maximum listed files (default: ${DEFAULT_MAX_FILES})
  --max-page-bytes <number> Maximum bytes per index page (default: ${DEFAULT_MAX_PAGE_BYTES})
  --max-in-flight-page-bytes <number>
                             Maximum aggregate bytes across one request batch
                             (default: ${DEFAULT_MAX_IN_FLIGHT_PAGE_BYTES})
  --concurrency <number>    Concurrent index requests (default: ${DEFAULT_CONCURRENCY})
  --snapshot-at <instant>   Override the ISO-8601 snapshot timestamp
  --help, -h                Show this help

Only same-origin directory index pages are fetched. Listed files and archives
are recorded as metadata; they are never fetched or extracted.`;
}

function positiveInteger(value, flag) {
  if (!/^\d+$/.test(value)) {
    throw new Error(`${flag} must be a positive integer`);
  }
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < 1) {
    throw new Error(`${flag} must be a positive safe integer`);
  }
  return parsed;
}

function optionValue(argv, index, flag) {
  const value = argv[index + 1];
  if (value === undefined || (value.startsWith("-") && value !== "-")) {
    throw new Error(`${flag} requires a value`);
  }
  return value;
}

export function parseCliArgs(argv) {
  const options = {
    root: DEFAULT_ROOT,
    timeoutMs: DEFAULT_TIMEOUT_MS,
    maxPages: DEFAULT_MAX_PAGES,
    maxFiles: DEFAULT_MAX_FILES,
    maxPageBytes: DEFAULT_MAX_PAGE_BYTES,
    maxInFlightPageBytes: DEFAULT_MAX_IN_FLIGHT_PAGE_BYTES,
    concurrency: DEFAULT_CONCURRENCY,
    snapshotAt: undefined,
    output: undefined,
    help: false,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--help" || argument === "-h") {
      options.help = true;
      continue;
    }

    const flag = argument === "-o" ? "--output" : argument;
    if (
      ![
        "--output",
        "--root",
        "--timeout-ms",
        "--max-pages",
        "--max-files",
        "--max-page-bytes",
        "--max-in-flight-page-bytes",
        "--concurrency",
        "--snapshot-at",
      ].includes(flag)
    ) {
      throw new Error(`unknown argument ${JSON.stringify(argument)}`);
    }

    const value = optionValue(argv, index, flag);
    index += 1;
    switch (flag) {
      case "--output":
        options.output = value;
        break;
      case "--root":
        options.root = value;
        break;
      case "--timeout-ms":
        options.timeoutMs = positiveInteger(value, flag);
        break;
      case "--max-pages":
        options.maxPages = positiveInteger(value, flag);
        break;
      case "--max-files":
        options.maxFiles = positiveInteger(value, flag);
        break;
      case "--max-page-bytes":
        options.maxPageBytes = positiveInteger(value, flag);
        break;
      case "--max-in-flight-page-bytes":
        options.maxInFlightPageBytes = positiveInteger(value, flag);
        break;
      case "--concurrency":
        options.concurrency = positiveInteger(value, flag);
        break;
      case "--snapshot-at":
        options.snapshotAt = value;
        break;
    }
  }

  if (!options.help && options.output === undefined) {
    throw new Error("--output is required");
  }
  return options;
}

function normalizeRootUrl(value) {
  let root;
  try {
    root = new URL(value);
  } catch (error) {
    throw new Error(`invalid index root ${JSON.stringify(value)}: ${error.message}`);
  }
  if (root.protocol !== "http:" && root.protocol !== "https:") {
    throw new Error("index root must use HTTP or HTTPS");
  }
  if (root.username || root.password) {
    throw new Error("index root must not contain credentials");
  }
  root.search = "";
  root.hash = "";
  if (!root.pathname.endsWith("/")) {
    root.pathname += "/";
  }
  return root;
}

function normalizeSnapshotAt(value) {
  const date = value === undefined ? new Date() : new Date(value);
  if (Number.isNaN(date.getTime())) {
    throw new Error(`invalid snapshot timestamp ${JSON.stringify(value)}`);
  }
  return date.toISOString();
}

function decodeHtmlAttribute(value) {
  return value.replace(
    /&(?:#(\d+)|#x([\da-f]+)|amp|quot|apos|lt|gt);/gi,
    (entity, decimal, hexadecimal) => {
      if (decimal !== undefined) return String.fromCodePoint(Number(decimal));
      if (hexadecimal !== undefined) return String.fromCodePoint(Number.parseInt(hexadecimal, 16));
      switch (entity.toLowerCase()) {
        case "&amp;":
          return "&";
        case "&quot;":
          return '"';
        case "&apos;":
          return "'";
        case "&lt;":
          return "<";
        case "&gt;":
          return ">";
        default:
          return entity;
      }
    },
  );
}

function rawPathFromReference(reference) {
  const withoutQuery = reference.split(/[?#]/, 1)[0];
  const absolute = withoutQuery.match(/^[a-z][a-z\d+.-]*:\/\/[^/]*(\/.*)?$/i);
  if (absolute) return absolute[1] ?? "/";
  const schemeRelative = withoutQuery.match(/^\/\/[^/]*(\/.*)?$/);
  return schemeRelative ? (schemeRelative[1] ?? "/") : withoutQuery;
}

function rejectTraversalReference(reference, pageUrl) {
  const rawPath = rawPathFromReference(reference);
  const segments = rawPath.split("/");
  for (let index = 0; index < segments.length; index += 1) {
    let decoded;
    try {
      decoded = decodeURIComponent(segments[index]);
    } catch {
      throw new Error(`${pageUrl}: listed href has invalid percent encoding: ${reference}`);
    }
    const leadingCurrentDirectory = index === 0 && decoded === "." && rawPath.startsWith("./");
    if (decoded === ".." || (decoded === "." && !leadingCurrentDirectory)) {
      throw new Error(`${pageUrl}: listed href contains path traversal: ${reference}`);
    }
    if (decoded.includes("\\") || decoded.includes("/") || decoded.includes("\0")) {
      throw new Error(`${pageUrl}: listed href contains an unsafe path segment: ${reference}`);
    }
  }
}

function resolveListedUrl(reference, pageUrl, rootUrl) {
  rejectTraversalReference(reference, pageUrl);
  let target;
  try {
    target = new URL(reference, pageUrl);
  } catch (error) {
    throw new Error(`${pageUrl}: invalid listed href ${JSON.stringify(reference)}: ${error.message}`);
  }
  if (target.origin !== rootUrl.origin) {
    throw new Error(`${pageUrl}: listed href must remain on origin ${rootUrl.origin}: ${reference}`);
  }
  if (!target.pathname.startsWith(rootUrl.pathname)) {
    throw new Error(`${pageUrl}: listed href escapes index root ${rootUrl.href}: ${reference}`);
  }
  target.hash = "";
  return target;
}

function decodedRelativePath(target, rootUrl, { directory = false } = {}) {
  let encoded = target.pathname.slice(rootUrl.pathname.length);
  if (directory && encoded.endsWith("/")) encoded = encoded.slice(0, -1);
  if (!encoded) return "";

  const decoded = encoded.split("/").map((segment) => {
    let value;
    try {
      value = decodeURIComponent(segment);
    } catch {
      throw new Error(`${target.href}: path has invalid percent encoding`);
    }
    if (
      !value ||
      value === "." ||
      value === ".." ||
      value.includes("/") ||
      value.includes("\\") ||
      value.includes("\0")
    ) {
      throw new Error(`${target.href}: path contains an unsafe decoded segment`);
    }
    return value;
  });
  return decoded.join("/");
}

function fileType(relativePath) {
  const name = relativePath.slice(relativePath.lastIndexOf("/") + 1);
  const dot = name.lastIndexOf(".");
  return dot > 0 && dot < name.length - 1 ? name.slice(dot + 1).toLowerCase() : "other";
}

function compareStrings(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function parseIndexPage(html, pageUrl, rootUrl) {
  const rows = [];
  const rowPattern = /<tr\b([^>]*)>([\s\S]*?)<\/tr\s*>/gi;
  for (const match of html.matchAll(rowPattern)) {
    const attributes = match[1];
    if (!/\bclass\s*=\s*(?:"[^"]*\bfile\b[^"]*"|'[^']*\bfile\b[^']*')/i.test(attributes)) {
      continue;
    }
    const body = match[2];
    const anchor = body.match(/<a\b[^>]*\bhref\s*=\s*(["'])([\s\S]*?)\1/i);
    const order = body.match(/<td\b[^>]*\bdata-order\s*=\s*(["'])(-?\d+)\1/i);
    if (!anchor || !order) {
      throw new Error(`${pageUrl}: malformed auto-index row`);
    }

    const reference = decodeHtmlAttribute(anchor[2]);
    const numericOrder = Number(order[2]);
    if (!Number.isSafeInteger(numericOrder) || numericOrder < -1) {
      throw new Error(`${pageUrl}: invalid data-order ${JSON.stringify(order[2])}`);
    }
    const target = resolveListedUrl(reference, pageUrl, rootUrl);

    if (numericOrder === -1) {
      target.search = "";
      if (!target.pathname.endsWith("/") || !target.pathname.startsWith(pageUrl.pathname)) {
        throw new Error(`${pageUrl}: listed directory is not a descendant: ${reference}`);
      }
      rows.push({ kind: "directory", url: target });
      continue;
    }

    if (target.pathname.endsWith("/")) {
      throw new Error(`${pageUrl}: listed file URL ends with /: ${reference}`);
    }
    const pageRelative = target.pathname.slice(pageUrl.pathname.length);
    if (!target.pathname.startsWith(pageUrl.pathname) || pageRelative.includes("/")) {
      throw new Error(`${pageUrl}: listed file is not a direct child: ${reference}`);
    }
    const relativePath = decodedRelativePath(target, rootUrl);
    const slash = relativePath.lastIndexOf("/");
    rows.push({
      kind: "file",
      file: {
        path: relativePath,
        type: fileType(relativePath),
        bytes: numericOrder,
        url: target.href,
        parent: slash === -1 ? "" : relativePath.slice(0, slash),
      },
    });
  }
  return rows;
}

async function readBoundedText(response, maximumBytes, url) {
  const declared = response.headers.get("content-length");
  if (declared !== null && /^\d+$/.test(declared) && Number(declared) > maximumBytes) {
    throw new Error(`${url}: index page exceeds --max-page-bytes (${declared} > ${maximumBytes})`);
  }
  if (!response.body) return "";

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let bytes = 0;
  let text = "";
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    bytes += value.byteLength;
    if (bytes > maximumBytes) {
      await reader.cancel();
      throw new Error(`${url}: index page exceeds --max-page-bytes (${bytes} > ${maximumBytes})`);
    }
    text += decoder.decode(value, { stream: true });
  }
  return text + decoder.decode();
}

async function fetchIndexPage(url, { fetchImpl, timeoutMs, maxPageBytes, rootUrl }) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetchImpl(url.href, {
      redirect: "manual",
      signal: controller.signal,
      headers: { "user-agent": "mdictlib-corpus-inventory/1" },
    });
    if (response.status >= 300 && response.status < 400) {
      throw new Error(`${url.href}: index redirects are not followed`);
    }
    if (!response.ok) throw new Error(`${url.href}: HTTP ${response.status}`);

    const finalUrl = new URL(response.url || url.href);
    if (finalUrl.origin !== rootUrl.origin || !finalUrl.pathname.startsWith(rootUrl.pathname)) {
      throw new Error(`${url.href}: response redirected outside index root`);
    }
    const contentType = response.headers.get("content-type") ?? "";
    if (!/^(?:text\/html|application\/xhtml\+xml)\b/i.test(contentType)) {
      throw new Error(`${url.href}: expected an HTML index page, got ${contentType || "no content type"}`);
    }
    return await readBoundedText(response, maxPageBytes, url.href);
  } catch (error) {
    if (controller.signal.aborted) {
      throw new Error(`${url.href}: index request timed out after ${timeoutMs} ms`, { cause: error });
    }
    throw error;
  } finally {
    clearTimeout(timer);
  }
}

function validateLimit(value, name) {
  if (!Number.isSafeInteger(value) || value < 1) {
    throw new Error(`${name} must be a positive safe integer`);
  }
}

export async function inventoryMdictIndex({
  root = DEFAULT_ROOT,
  timeoutMs = DEFAULT_TIMEOUT_MS,
  maxPages = DEFAULT_MAX_PAGES,
  maxFiles = DEFAULT_MAX_FILES,
  maxPageBytes = DEFAULT_MAX_PAGE_BYTES,
  maxInFlightPageBytes = DEFAULT_MAX_IN_FLIGHT_PAGE_BYTES,
  concurrency = DEFAULT_CONCURRENCY,
  snapshotAt,
  fetchImpl = globalThis.fetch,
} = {}) {
  if (typeof fetchImpl !== "function") throw new Error("a fetch implementation is required");
  validateLimit(timeoutMs, "timeoutMs");
  validateLimit(maxPages, "maxPages");
  validateLimit(maxFiles, "maxFiles");
  validateLimit(maxPageBytes, "maxPageBytes");
  validateLimit(maxInFlightPageBytes, "maxInFlightPageBytes");
  validateLimit(concurrency, "concurrency");
  if (maxPageBytes > Math.floor(maxInFlightPageBytes / concurrency)) {
    throw new Error(
      `concurrency * maxPageBytes exceeds the maxInFlightPageBytes limit of ${maxInFlightPageBytes}`,
    );
  }

  const rootUrl = normalizeRootUrl(root);
  const normalizedSnapshotAt = normalizeSnapshotAt(snapshotAt);
  const queued = new Set([rootUrl.href]);
  const visited = new Set();
  const pending = [rootUrl.href];
  const filesByPath = new Map();

  while (pending.length > 0) {
    pending.sort(compareStrings);
    if (visited.size >= maxPages) {
      throw new Error(`index exceeds maxPages limit of ${maxPages}`);
    }
    const capacity = Math.min(concurrency, maxPages - visited.size);
    const batch = pending.splice(0, capacity).filter((url) => !visited.has(url));
    batch.forEach((url) => visited.add(url));
    const pages = await Promise.all(
      batch.map(async (url) => {
        const pageUrl = new URL(url);
        const html = await fetchIndexPage(pageUrl, {
          fetchImpl,
          timeoutMs,
          maxPageBytes,
          rootUrl,
        });
        return { pageUrl, rows: parseIndexPage(html, pageUrl, rootUrl) };
      }),
    );

    for (const { rows } of pages) {
      for (const row of rows) {
        if (row.kind === "directory") {
          if (!queued.has(row.url.href)) {
            queued.add(row.url.href);
            pending.push(row.url.href);
          }
          continue;
        }
        const previous = filesByPath.get(row.file.path);
        if (previous && JSON.stringify(previous) !== JSON.stringify(row.file)) {
          throw new Error(`conflicting file rows for ${JSON.stringify(row.file.path)}`);
        }
        filesByPath.set(row.file.path, row.file);
        if (filesByPath.size > maxFiles) {
          throw new Error(`index exceeds maxFiles limit of ${maxFiles}`);
        }
      }
    }
  }

  const files = [...filesByPath.values()].sort(
    (left, right) => compareStrings(left.path, right.path) || compareStrings(left.url, right.url),
  );
  let advertisedBytes = 0;
  for (const file of files) {
    advertisedBytes += file.bytes;
    if (!Number.isSafeInteger(advertisedBytes)) {
      throw new Error("aggregate advertised byte count exceeds the safe integer range");
    }
  }

  return {
    schemaVersion: 1,
    root: rootUrl.href,
    snapshotAt: normalizedSnapshotAt,
    pageCount: visited.size,
    fileCount: files.length,
    advertisedBytes,
    files,
  };
}

async function writeInventory(output, inventory) {
  const json = `${JSON.stringify(inventory, null, 2)}\n`;
  if (output === "-") {
    process.stdout.write(json);
    return;
  }

  const destination = path.resolve(output);
  await mkdir(path.dirname(destination), { recursive: true });
  const temporary = path.join(
    path.dirname(destination),
    `.${path.basename(destination)}.tmp-${process.pid}-${randomUUID()}`,
  );
  try {
    await writeFile(temporary, json, { encoding: "utf8", flag: "wx" });
    await rename(temporary, destination);
  } catch (error) {
    await rm(temporary, { force: true });
    throw error;
  }
}

export async function runCli(argv = process.argv.slice(2)) {
  const options = parseCliArgs(argv);
  if (options.help) {
    process.stdout.write(`${usage()}\n`);
    return;
  }
  const inventory = await inventoryMdictIndex(options);
  await writeInventory(options.output, inventory);
  if (options.output !== "-") {
    process.stdout.write(
      `Inventoried ${inventory.fileCount} files across ${inventory.pageCount} pages -> ${path.resolve(options.output)}\n`,
    );
  }
}

const invokedPath = process.argv[1] ? pathToFileURL(path.resolve(process.argv[1])).href : undefined;
if (invokedPath === import.meta.url) {
  runCli().catch((error) => {
    process.stderr.write(`inventory-mdict-index: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}

export const scriptPath = fileURLToPath(import.meta.url);
