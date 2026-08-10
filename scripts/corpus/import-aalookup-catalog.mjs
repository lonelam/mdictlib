#!/usr/bin/env node

import { createHash } from "node:crypto";
import { pathToFileURL } from "node:url";
import process from "node:process";
import {
  assertExactKeys,
  assertDistinctPaths,
  compareText,
  fail,
  parseOptions,
  readJson,
  requireBoolean,
  requireHttpUrl,
  requireString,
  sha256File,
  stableJson,
  writeTextAtomic,
} from "./lib.mjs";

const SCOPE_NOTE =
  "Discovery candidates from one AALookup agent draft. The upstream generator explicitly stops after a reasonable set; this is not an exhaustive site or Internet inventory, and URL availability is not license approval.";

function shortHash(value) {
  return createHash("sha256").update(value).digest("hex").slice(0, 16);
}

function classifyUrl(url) {
  const pathname = new URL(url).pathname.toLowerCase();
  const match = pathname.match(/\.([^.\/]+)$/);
  const extension = match?.[1] ?? "";
  if (extension === "mdx" || extension === "mdd" || extension === "css") return extension;
  if (extension === "js" || extension === "mjs" || extension === "cjs") return "javascript";
  if (["zip", "7z", "rar", "tar", "gz", "tgz", "bz2", "xz"].includes(extension)) return "archive";
  return "other";
}

function canonicalUrl(raw, where) {
  const parsed = requireHttpUrl(raw, where);
  const url = new URL(parsed);
  url.hash = "";
  return url.toString();
}

export function importAalookupCatalog(raw, sourceSha256) {
  if (!Array.isArray(raw)) fail("AALookup draft must be a JSON array");
  const entryMap = new Map();
  const urlMap = new Map();

  raw.forEach((entry, index) => {
    const where = `draft[${index}]`;
    const fields = ["title", "type", "url", "downloadable", "downloadUrls"];
    assertExactKeys(entry, fields, fields, where);
    const title = requireString(entry.title, `${where}.title`);
    const declaredType = requireString(entry.type, `${where}.type`);
    requireBoolean(entry.downloadable, `${where}.downloadable`);
    if (!Array.isArray(entry.downloadUrls)) fail(`${where}.downloadUrls must be an array`);
    if (entry.downloadable && entry.downloadUrls.length === 0) {
      fail(`${where} is downloadable but has no downloadUrls`);
    }
    const occurrences = [
      { raw: entry.url, role: "info" },
      ...entry.downloadUrls.map((rawUrl, downloadIndex) => ({
        raw: requireString(rawUrl, `${where}.downloadUrls[${downloadIndex}]`),
        role: "download",
      })),
    ];
    const normalized = occurrences.map(({ raw: rawUrl, role }) => ({
      url: canonicalUrl(rawUrl, `${where} ${role} URL`),
      role,
    }));
    const signature = stableJson({
      declaredType,
      downloadable: entry.downloadable,
      title,
      urls: normalized,
    });
    const entryId = `candidate-${shortHash(signature)}`;
    let candidate = entryMap.get(entryId);
    if (!candidate) {
      candidate = {
        declaredType,
        downloadable: entry.downloadable,
        id: entryId,
        occurrences: 0,
        title,
        urlIds: [],
      };
      entryMap.set(entryId, candidate);
    }
    candidate.occurrences += 1;

    for (const { url, role } of normalized) {
      const urlId = `url-${shortHash(url)}`;
      if (!candidate.urlIds.includes(urlId)) candidate.urlIds.push(urlId);
      let row = urlMap.get(url);
      if (!row) {
        row = {
          classification: classifyUrl(url),
          id: urlId,
          references: [],
          roles: [],
          url,
        };
        urlMap.set(url, row);
      }
      if (!row.roles.includes(role)) row.roles.push(role);
      const reference = `${entryId}:${role}`;
      if (!row.references.includes(reference)) row.references.push(reference);
    }
  });

  const entries = [...entryMap.values()]
    .map((entry) => ({ ...entry, urlIds: entry.urlIds.sort() }))
    .sort((left, right) => compareText(left.id, right.id));
  const urls = [...urlMap.values()]
    .map((row) => ({
      ...row,
      references: row.references.sort(),
      roles: row.roles.sort(),
    }))
    .sort((left, right) => compareText(left.url, right.url));
  return {
    entries,
    schemaVersion: 1,
    source: {
      kind: "aalookup-agent-draft",
      scope: SCOPE_NOTE,
      sha256: sourceSha256,
    },
    urls,
  };
}

export async function main(argv = process.argv.slice(2)) {
  const options = parseOptions(argv, { "--input": "string", "--output": "string" });
  if (!options.input || !options.output) {
    fail("usage: import-aalookup-catalog.mjs --input <draft.json> --output <candidates.json>");
  }
  await assertDistinctPaths({ input: options.input, output: options.output });
  const [raw, identityBefore] = await Promise.all([readJson(options.input), sha256File(options.input)]);
  const identityAfter = await sha256File(options.input);
  if (identityBefore.bytes !== identityAfter.bytes || identityBefore.sha256 !== identityAfter.sha256) {
    fail("AALookup draft changed while it was being read");
  }
  const sourceSha256 = identityBefore.sha256;
  const result = importAalookupCatalog(raw, sourceSha256);
  await writeTextAtomic(options.output, stableJson(result));
  process.stdout.write(
    `Imported ${result.entries.length} deduplicated candidate entries and ${result.urls.length} classified URLs.\n` +
      `${SCOPE_NOTE}\n`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`import-aalookup-catalog: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
