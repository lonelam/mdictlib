import { createHash, randomBytes } from "node:crypto";
import { lookup } from "node:dns/promises";
import { constants } from "node:fs";
import http from "node:http";
import https from "node:https";
import { homedir } from "node:os";
import {
  lstat,
  link,
  mkdir,
  open,
  realpath,
  rename,
  rm,
  stat,
} from "node:fs/promises";
import path from "node:path";
import { BlockList, isIP } from "node:net";

export const MANIFEST_NAME = "mdictlib-corpus.tsv";
export const MANIFEST_HEADER =
  "path\tkind\tbytes\tsha256\tentries\tkey_sha256\tpayload_sha256";
export const DEFAULT_MAX_FILE_BYTES = 8 * 1024 * 1024 * 1024;
export const DEFAULT_MAX_TOTAL_BYTES = 64 * 1024 * 1024 * 1024;

const SHA256 = /^[0-9a-f]{64}$/;
const ENTRY_ID = /^[a-z0-9](?:[a-z0-9._-]{0,126}[a-z0-9])?$/;
const MAX_JSON_BYTES = 64 * 1024 * 1024;
const NOFOLLOW = constants.O_NOFOLLOW ?? 0;

// Fail closed against the IANA special-purpose registries. IPv6 is accepted
// only from the Global Unicast blocks whose IANA status was ALLOCATED in the
// 2025-10-10 registry snapshot, with special-purpose subranges removed. IANA
// reserves every unlisted part of 2000::/3 for future allocation.
const NON_PUBLIC_IPV4 = new BlockList();
for (const [network, prefix] of [
  ["0.0.0.0", 8],
  ["10.0.0.0", 8],
  ["100.64.0.0", 10],
  ["127.0.0.0", 8],
  ["169.254.0.0", 16],
  ["172.16.0.0", 12],
  ["192.0.0.0", 24],
  ["192.0.2.0", 24],
  ["192.31.196.0", 24],
  ["192.52.193.0", 24],
  ["192.88.99.0", 24],
  ["192.168.0.0", 16],
  ["192.175.48.0", 24],
  ["198.18.0.0", 15],
  ["198.51.100.0", 24],
  ["203.0.113.0", 24],
  ["224.0.0.0", 3],
]) {
  NON_PUBLIC_IPV4.addSubnet(network, prefix, "ipv4");
}

const PUBLIC_IPV6 = new BlockList();
for (const [network, prefix] of [
  ["2001:200::", 23],
  ["2001:400::", 23],
  ["2001:600::", 23],
  ["2001:800::", 22],
  ["2001:c00::", 23],
  ["2001:e00::", 23],
  ["2001:1200::", 23],
  ["2001:1400::", 22],
  ["2001:1800::", 23],
  ["2001:1a00::", 23],
  ["2001:1c00::", 22],
  ["2001:2000::", 19],
  ["2001:4000::", 23],
  ["2001:4200::", 23],
  ["2001:4400::", 23],
  ["2001:4600::", 23],
  ["2001:4800::", 23],
  ["2001:4a00::", 23],
  ["2001:4c00::", 23],
  ["2001:5000::", 20],
  ["2001:8000::", 19],
  ["2001:a000::", 20],
  ["2001:b000::", 20],
  ["2003::", 18],
  ["2400::", 12],
  ["2410::", 12],
  ["2600::", 12],
  ["2610::", 23],
  ["2620::", 23],
  ["2630::", 12],
  ["2800::", 12],
  ["2a00::", 12],
  ["2a10::", 12],
  ["2c00::", 12],
]) {
  PUBLIC_IPV6.addSubnet(network, prefix, "ipv6");
}
const NON_PUBLIC_IPV6 = new BlockList();
for (const [network, prefix] of [
  ["2001::", 23],
  ["2001:db8::", 32],
  ["2002::", 16],
  ["3fff::", 20],
]) {
  NON_PUBLIC_IPV6.addSubnet(network, prefix, "ipv6");
}

export function fail(message) {
  throw new Error(message);
}

export async function readJson(jsonPath) {
  let handle;
  let value;
  try {
    handle = await open(jsonPath, constants.O_RDONLY | NOFOLLOW);
    const metadata = await handle.stat();
    if (!metadata.isFile()) fail(`${jsonPath} is not a regular file`);
    if (metadata.size > MAX_JSON_BYTES) {
      fail(`${jsonPath} is larger than the ${MAX_JSON_BYTES}-byte JSON limit`);
    }
    value = JSON.parse(await handle.readFile("utf8"));
  } catch (error) {
    fail(`failed to parse ${jsonPath}: ${error instanceof Error ? error.message : String(error)}`);
  } finally {
    await handle?.close();
  }
  return value;
}

export function stableJson(value) {
  return `${JSON.stringify(sortObject(value), null, 2)}\n`;
}

function sortObject(value) {
  if (Array.isArray(value)) return value.map(sortObject);
  if (value === null || typeof value !== "object") return value;
  return Object.fromEntries(
    Object.keys(value)
      .sort()
      .map((key) => [key, sortObject(value[key])]),
  );
}

export function sha256Text(value) {
  return createHash("sha256").update(value).digest("hex");
}

export function sanitizeDiagnostic(value, {
  workspaceRoot = null,
  corpusRoot = null,
  target = null,
  observerPath = null,
  homeRoot = homedir(),
  maxBytes = 4_096,
} = {}) {
  let sanitized = String(value)
    .replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g, "?");
  const replacements = [
    [target, "<corpus-artifact>"],
    [observerPath, "<observer>"],
    [corpusRoot, "<corpus-root>"],
    [workspaceRoot, "<workspace>"],
    [homeRoot, "<home>"],
  ]
    .filter(([candidate]) => typeof candidate === "string" && candidate !== "")
    .map(([candidate, replacement]) => [path.resolve(candidate), replacement])
    .sort(([left], [right]) => right.length - left.length);
  for (const [candidate, replacement] of replacements) {
    sanitized = sanitized.replaceAll(candidate, replacement);
    if (path.sep === "\\") {
      sanitized = sanitized.replaceAll(candidate.replaceAll("\\", "/"), replacement);
    }
  }
  return sanitized.slice(0, maxBytes);
}

export async function writeTextAtomic(outputPath, text) {
  const outputDirectory = path.dirname(path.resolve(outputPath));
  await mkdir(outputDirectory, { recursive: true });
  const temporary = path.join(
    outputDirectory,
    `${path.basename(outputPath)}.part-${process.pid}-${randomBytes(8).toString("hex")}`,
  );
  let handle;
  try {
    handle = await open(temporary, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | NOFOLLOW, 0o600);
    await handle.writeFile(text, "utf8");
    await handle.sync();
    await handle.close();
    handle = undefined;
    await rename(temporary, outputPath);
    let directoryHandle;
    try {
      directoryHandle = await open(outputDirectory, constants.O_RDONLY);
      await directoryHandle.sync();
    } catch (error) {
      if (!["EINVAL", "ENOTSUP", "EBADF", "EISDIR"].includes(error?.code)) throw error;
    } finally {
      await directoryHandle?.close();
    }
  } catch (error) {
    await handle?.close();
    await rm(temporary, { force: true });
    throw error;
  }
}

export async function sha256File(filePath) {
  const hash = createHash("sha256");
  const bytes = await updateHashFromFile(filePath, hash);
  return { bytes, sha256: hash.digest("hex") };
}

async function updateHashFromFile(filePath, hash, signal = null) {
  let bytes = 0;
  let handle;
  try {
    handle = await open(filePath, constants.O_RDONLY | NOFOLLOW);
    const metadata = await handle.stat();
    if (!metadata.isFile()) fail(`${filePath} is not a regular file`);
    const buffer = Buffer.allocUnsafe(1024 * 1024);
    while (true) {
      if (signal?.aborted) throw signal.reason ?? new Error("operation aborted while hashing partial download");
      const { bytesRead } = await handle.read(buffer, 0, buffer.length, bytes);
      if (bytesRead === 0) break;
      hash.update(buffer.subarray(0, bytesRead));
      bytes += bytesRead;
    }
  } finally {
    await handle?.close();
  }
  return bytes;
}

async function updateHashFromHandle(handle, hash, expectedIdentity, signal = null) {
  let bytes = 0;
  const buffer = Buffer.allocUnsafe(1024 * 1024);
  while (true) {
    if (signal?.aborted) throw signal.reason ?? new Error("operation aborted while hashing partial download");
    const { bytesRead } = await handle.read(buffer, 0, buffer.length, bytes);
    if (bytesRead === 0) break;
    hash.update(buffer.subarray(0, bytesRead));
    bytes += bytesRead;
  }
  const after = await handle.stat({ bigint: true });
  if (
    !after.isFile() ||
    after.nlink !== expectedIdentity.nlink ||
    after.dev !== expectedIdentity.dev ||
    after.ino !== expectedIdentity.ino ||
    after.size !== expectedIdentity.size
  ) {
    fail("partial download changed while it was being hashed");
  }
  return bytes;
}

function requirePartialIdentity(metadata, expectedBytes, where = "partial download") {
  if (!metadata.isFile() || metadata.nlink !== 1n) {
    fail(`${where} must be a singly linked regular file`);
  }
  if (metadata.size !== BigInt(expectedBytes)) {
    fail(`${where} changed size while it was open`);
  }
  return { dev: metadata.dev, ino: metadata.ino, nlink: metadata.nlink, size: metadata.size };
}

export function assertRecord(value, where) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    fail(`${where} must be an object`);
  }
}

export function assertExactKeys(value, allowed, required, where) {
  assertRecord(value, where);
  const allowedSet = new Set(allowed);
  for (const key of Object.keys(value)) {
    if (!allowedSet.has(key)) fail(`${where} has unknown field ${JSON.stringify(key)}`);
  }
  for (const key of required) {
    if (!Object.hasOwn(value, key)) fail(`${where} is missing field ${JSON.stringify(key)}`);
  }
}

export function requireString(value, where, { nullable = false } = {}) {
  if (nullable && value === null) return null;
  if (typeof value !== "string" || value.trim() === "") fail(`${where} must be a non-empty string`);
  if (value.includes("\0")) fail(`${where} must not contain NUL`);
  return value;
}

export function requireBoolean(value, where) {
  if (typeof value !== "boolean") fail(`${where} must be a boolean`);
  return value;
}

export function requireSafeInteger(value, where, minimum = 0) {
  if (!Number.isSafeInteger(value) || value < minimum) {
    fail(`${where} must be a safe integer >= ${minimum}`);
  }
  return value;
}

export function requireHttpUrl(value, where) {
  requireString(value, where);
  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    fail(`${where} must be an absolute HTTP(S) URL`);
  }
  if (!['http:', 'https:'].includes(parsed.protocol) || parsed.username || parsed.password) {
    fail(`${where} must be an absolute HTTP(S) URL without embedded credentials`);
  }
  return parsed.toString();
}

function normalizedHostname(url) {
  return url.hostname.startsWith("[") && url.hostname.endsWith("]")
    ? url.hostname.slice(1, -1)
    : url.hostname;
}

export function isPublicIpAddress(address) {
  const version = isIP(address);
  if (version === 4) {
    return !NON_PUBLIC_IPV4.check(address, "ipv4");
  }
  if (version === 6) {
    return PUBLIC_IPV6.check(address, "ipv6") && !NON_PUBLIC_IPV6.check(address, "ipv6");
  }
  return false;
}

function isMetadataHostname(hostname) {
  const lower = hostname.toLowerCase().replace(/\.$/, "");
  return (
    lower === "localhost" ||
    lower.endsWith(".localhost") ||
    lower === "metadata.google.internal" ||
    lower === "metadata.azure.internal" ||
    lower === "instance-data.ec2.internal"
  );
}

export function requireAcquisitionUrl(
  value,
  where,
  { allowPrivateAddresses = false, allowInsecureHttp = false } = {},
) {
  const parsed = new URL(requireHttpUrl(value, where));
  if (!allowInsecureHttp && parsed.protocol !== "https:") {
    fail(`${where} must use HTTPS`);
  }
  if (parsed.search !== "" || parsed.hash !== "") {
    fail(`${where} must not contain a query string or fragment`);
  }
  const hostname = normalizedHostname(parsed);
  if (!allowPrivateAddresses) {
    if (isMetadataHostname(hostname)) fail(`${where} uses a local or cloud-metadata hostname`);
    const version = isIP(hostname);
    if (version !== 0 && !isPublicIpAddress(hostname)) {
      fail(`${where} uses a non-public IP address`);
    }
    if (version === 0 && !hostname.includes(".")) {
      fail(`${where} hostname must be a public fully-qualified name`);
    }
  }
  return parsed.toString();
}

async function pinnedLookupFor(url, policy, signal) {
  if (policy.allowPrivateAddresses) return undefined;
  const hostname = normalizedHostname(url);
  if (isIP(hostname) !== 0) {
    if (!isPublicIpAddress(hostname)) fail("download target resolved to a non-public address");
    return undefined;
  }
  let addresses;
  try {
    addresses = await abortable(lookup(hostname, { all: true, verbatim: true }), signal);
  } catch {
    if (signal.aborted) throw signal.reason ?? new Error("download aborted while resolving hostname");
    fail("download hostname did not resolve");
  }
  if (addresses.length === 0 || addresses.some(({ address }) => !isPublicIpAddress(address))) {
    fail("download hostname did not resolve exclusively to public addresses");
  }
  const selected = addresses.find(({ family }) => family === 4) ?? addresses[0];
  return (requestedHostname, options, callback) => {
    if (requestedHostname.toLowerCase().replace(/\.$/, "") !== hostname.toLowerCase().replace(/\.$/, "")) {
      callback(new Error("download attempted to resolve an unexpected hostname"));
      return;
    }
    if (options?.all) {
      callback(null, [{ address: selected.address, family: selected.family }]);
    } else {
      callback(null, selected.address, selected.family);
    }
  };
}

async function abortable(promise, signal) {
  if (signal.aborted) throw signal.reason ?? new Error("operation aborted");
  let onAbort;
  const aborted = new Promise((_, reject) => {
    onAbort = () => reject(signal.reason ?? new Error("operation aborted"));
    signal.addEventListener("abort", onAbort, { once: true });
  });
  try {
    return await Promise.race([promise, aborted]);
  } finally {
    signal.removeEventListener("abort", onAbort);
  }
}

function getResponseHeader(response, name) {
  const value = response.headers[name.toLowerCase()];
  return Array.isArray(value) ? value.join(", ") : (value ?? null);
}

async function requestOnce(url, { headers, lookupFunction, signal }) {
  const parsed = new URL(url);
  const transport = parsed.protocol === "https:" ? https : http;
  return new Promise((resolve, reject) => {
    const request = transport.request(parsed, {
      headers,
      lookup: lookupFunction,
      method: "GET",
      signal,
    });
    request.once("error", reject);
    request.once("response", (response) => {
      const status = response.statusCode ?? 0;
      resolve({
        body: {
          cancel: async () => response.destroy(),
          [Symbol.asyncIterator]: () => response[Symbol.asyncIterator](),
        },
        headers: { get: (name) => getResponseHeader(response, name) },
        ok: status >= 200 && status < 300,
        status,
      });
    });
    request.end();
  });
}

export function requireSameOriginRedirect(location, currentUrl, initialUrl, policy = {}) {
  let resolved;
  try {
    resolved = new URL(location, currentUrl);
  } catch {
    fail("redirect Location is not a valid URL");
  }
  const checked = new URL(requireAcquisitionUrl(resolved.toString(), "redirect URL", policy));
  if (checked.origin !== new URL(initialUrl).origin) {
    fail("redirect must remain on the reviewed URL origin");
  }
  return checked.toString();
}

export function requireRelativePath(value, kind, where) {
  requireString(value, where);
  if (value.includes("\\") || value.includes("\t") || value.includes("\r") || value.includes("\n")) {
    fail(`${where} must be a normalized POSIX path without control separators`);
  }
  if (
    value.startsWith("/") ||
    /^[A-Za-z]:/.test(value) ||
    path.posix.normalize(value) !== value ||
    value.split("/").some((part) => part === "" || part === "." || part === "..")
  ) {
    fail(`${where} must be a normalized relative POSIX path`);
  }
  if (path.posix.extname(value).slice(1).toLowerCase() !== kind) {
    fail(`${where} must end in .${kind}`);
  }
  return value;
}

export function validateReview(review, where) {
  const fields = [
    "status",
    "testingAllowed",
    "redistributionAllowed",
    "license",
    "licenseUrl",
    "reviewedBy",
    "reviewedAt",
    "notes",
  ];
  assertExactKeys(review, fields, fields, where);
  if (!["approved", "rejected", "pending"].includes(review.status)) {
    fail(`${where}.status must be approved, rejected, or pending`);
  }
  requireBoolean(review.testingAllowed, `${where}.testingAllowed`);
  requireBoolean(review.redistributionAllowed, `${where}.redistributionAllowed`);
  requireString(review.license, `${where}.license`);
  if (review.licenseUrl !== null) requireHttpUrl(review.licenseUrl, `${where}.licenseUrl`);
  if (review.reviewedBy !== null) requireString(review.reviewedBy, `${where}.reviewedBy`);
  if (review.reviewedAt !== null) {
    requireString(review.reviewedAt, `${where}.reviewedAt`);
    if (!Number.isFinite(Date.parse(review.reviewedAt))) fail(`${where}.reviewedAt is not an ISO date-time`);
  }
  if (typeof review.notes !== "string") fail(`${where}.notes must be a string`);

  if (review.status === "approved") {
    if (!review.testingAllowed) fail(`${where} is approved but testingAllowed is false`);
    if (review.reviewedBy === null || review.reviewedAt === null || review.notes.trim() === "") {
      fail(`${where} local-testing approval requires reviewedBy, reviewedAt, and non-empty notes`);
    }
  } else if (review.testingAllowed) {
    fail(`${where}.testingAllowed must be false unless status is approved`);
  }
  if (
    review.redistributionAllowed &&
    (review.licenseUrl === null || ["unknown", "unverified"].includes(review.license.trim().toLowerCase()))
  ) {
    fail(`${where} cannot allow redistribution without affirmative license evidence and licenseUrl`);
  }
  return review;
}

function validateCatalogMetadata(catalog, where = "catalog") {
  assertExactKeys(catalog, ["name", "scope"], ["name", "scope"], where);
  requireString(catalog.name, `${where}.name`);
  requireString(catalog.scope, `${where}.scope`);
}

function digestRows(rows) {
  const serialized = [...rows]
    .sort((left, right) => compareText(left, right))
    .map((row) => `${row}\n`)
    .join("");
  return sha256Text(serialized);
}

export function sourceRowSetSha256(rows) {
  return digestRows(
    rows.map(({ sourcePath, kind, advertisedBytes, url }) =>
      `${kind}\t${sourcePath}\t${advertisedBytes}\t${url}`),
  );
}

export function selectionArtifactSetSha256(rows) {
  return digestRows(
    rows.map(({ sourcePath, kind, path: localPath, advertisedBytes, url }) =>
      `${kind}\t${sourcePath}\t${localPath}\t${advertisedBytes}\t${url}`),
  );
}

export function validateSelectionSource(source, where = "selection.source", networkPolicy = {}) {
  const fields = [
    "kind",
    "inventorySha256",
    "root",
    "snapshotAt",
    "selectedType",
    "selectedCount",
    "selectedAdvertisedBytes",
    "selectedSetSha256",
  ];
  assertExactKeys(source, fields, fields, where);
  if (source.kind !== "mdict-index-inventory-v1") fail(`${where}.kind must be mdict-index-inventory-v1`);
  for (const field of ["inventorySha256", "selectedSetSha256"]) {
    if (typeof source[field] !== "string" || !SHA256.test(source[field])) {
      fail(`${where}.${field} must be 64 lowercase hexadecimal digits`);
    }
  }
  requireAcquisitionUrl(source.root, `${where}.root`, networkPolicy);
  requireString(source.snapshotAt, `${where}.snapshotAt`);
  if (!Number.isFinite(Date.parse(source.snapshotAt))) fail(`${where}.snapshotAt is not an ISO date-time`);
  if (!["mdx", "mdd"].includes(source.selectedType)) fail(`${where}.selectedType must be mdx or mdd`);
  requireSafeInteger(source.selectedCount, `${where}.selectedCount`, 1);
  requireSafeInteger(source.selectedAdvertisedBytes, `${where}.selectedAdvertisedBytes`, 1);
  return source;
}

export function validateSelectionBinding(binding, where = "selectionBinding", networkPolicy = {}) {
  const fields = [
    "advertisedBytes",
    "artifactCount",
    "artifactSetSha256",
    "entryCount",
    "selectionSha256",
    "source",
  ];
  assertExactKeys(binding, fields, fields, where);
  requireSafeInteger(binding.advertisedBytes, `${where}.advertisedBytes`, 1);
  requireSafeInteger(binding.artifactCount, `${where}.artifactCount`, 1);
  requireSafeInteger(binding.entryCount, `${where}.entryCount`, 1);
  for (const field of ["artifactSetSha256", "selectionSha256"]) {
    if (typeof binding[field] !== "string" || !SHA256.test(binding[field])) {
      fail(`${where}.${field} must be 64 lowercase hexadecimal digits`);
    }
  }
  validateSelectionSource(binding.source, `${where}.source`, networkPolicy);
  if (binding.advertisedBytes !== binding.source.selectedAdvertisedBytes) {
    fail(`${where}.advertisedBytes differs from its bound source selection`);
  }
  if (binding.artifactCount !== binding.source.selectedCount) {
    fail(`${where}.artifactCount differs from its bound source selection`);
  }
  if (binding.entryCount !== binding.artifactCount) {
    fail(`${where}.entryCount must equal artifactCount for one-artifact selection entries`);
  }
  return binding;
}

export function validateObserver(observer, where) {
  if (observer === null) return null;
  const fields = ["binaryBytes", "binarySha256", "mode", "timeoutMs", "tool", "version"];
  assertExactKeys(observer, fields, fields, where);
  requireSafeInteger(observer.binaryBytes, `${where}.binaryBytes`, 1);
  if (typeof observer.binarySha256 !== "string" || !SHA256.test(observer.binarySha256)) {
    fail(`${where}.binarySha256 must be 64 lowercase hexadecimal digits`);
  }
  if (observer.mode !== "metadata-open-and-count") {
    fail(`${where}.mode must be metadata-open-and-count`);
  }
  requireSafeInteger(observer.timeoutMs, `${where}.timeoutMs`, 1);
  requireString(observer.tool, `${where}.tool`);
  requireString(observer.version, `${where}.version`);
  return observer;
}

function validateEntryBase(entry, where, artifactValidator) {
  assertExactKeys(
    entry,
    ["id", "title", "infoUrl", "review", "artifacts"],
    ["id", "title", "infoUrl", "review", "artifacts"],
    where,
  );
  requireString(entry.id, `${where}.id`);
  if (!ENTRY_ID.test(entry.id)) fail(`${where}.id is not a stable lowercase identifier`);
  requireString(entry.title, `${where}.title`);
  requireHttpUrl(entry.infoUrl, `${where}.infoUrl`);
  validateReview(entry.review, `${where}.review`);
  if (!Array.isArray(entry.artifacts) || entry.artifacts.length === 0) {
    fail(`${where}.artifacts must be a non-empty array`);
  }
  entry.artifacts.forEach((artifact, index) => artifactValidator(artifact, `${where}.artifacts[${index}]`));
}

export function validateSelection(value, networkPolicy = {}) {
  assertExactKeys(
    value,
    ["schemaVersion", "source", "catalog", "entries"],
    ["schemaVersion", "source", "catalog", "entries"],
    "selection",
  );
  if (value.schemaVersion !== 1) fail("selection.schemaVersion must be 1");
  validateSelectionSource(value.source, "selection.source", networkPolicy);
  validateCatalogMetadata(value.catalog, "selection.catalog");
  if (!Array.isArray(value.entries)) fail("selection.entries must be an array");
  const ids = new Set();
  const paths = new Set();
  const sourcePaths = new Set();
  const allArtifacts = [];
  value.entries.forEach((entry, entryIndex) => {
    const where = `selection.entries[${entryIndex}]`;
    if (!Array.isArray(entry?.artifacts) || entry.artifacts.length !== 1) {
      fail(`${where}.artifacts must contain exactly one source artifact`);
    }
    validateEntryBase(entry, where, (artifact, artifactWhere) => {
      assertExactKeys(
        artifact,
        ["kind", "sourcePath", "url", "path", "advertisedBytes"],
        ["kind", "sourcePath", "url", "path", "advertisedBytes"],
        artifactWhere,
      );
      if (!["mdx", "mdd"].includes(artifact.kind)) fail(`${artifactWhere}.kind must be mdx or mdd`);
      if (artifact.kind !== value.source.selectedType) fail(`${artifactWhere}.kind differs from selection.source.selectedType`);
      requireString(artifact.sourcePath, `${artifactWhere}.sourcePath`);
      if (/[\t\r\n\0]/.test(artifact.sourcePath)) fail(`${artifactWhere}.sourcePath contains a forbidden control character`);
      requireAcquisitionUrl(artifact.url, `${artifactWhere}.url`, networkPolicy);
      requireRelativePath(artifact.path, artifact.kind, `${artifactWhere}.path`);
      requireSafeInteger(artifact.advertisedBytes, `${artifactWhere}.advertisedBytes`, 1);
      if (!paths.add(artifact.path)) fail(`${artifactWhere}.path duplicates ${artifact.path}`);
      if (!sourcePaths.add(artifact.sourcePath)) fail(`${artifactWhere}.sourcePath duplicates ${artifact.sourcePath}`);
      allArtifacts.push(artifact);
    });
    if (!ids.add(entry.id)) fail(`${where}.id duplicates ${entry.id}`);
    if (entry.review.status !== "approved") fail(`${where} must have approved local testing review before download`);
  });
  const advertisedBytes = allArtifacts.reduce((sum, artifact) => sum + artifact.advertisedBytes, 0);
  if (!Number.isSafeInteger(advertisedBytes)) fail("selection advertised byte total exceeds the safe integer range");
  if (allArtifacts.length !== value.source.selectedCount) {
    fail(`selection contains ${allArtifacts.length} artifacts; source declares ${value.source.selectedCount}`);
  }
  if (advertisedBytes !== value.source.selectedAdvertisedBytes) {
    fail(`selection advertises ${advertisedBytes} bytes; source declares ${value.source.selectedAdvertisedBytes}`);
  }
  const selectedSetSha256 = sourceRowSetSha256(allArtifacts);
  if (selectedSetSha256 !== value.source.selectedSetSha256) {
    fail("selection artifact set does not match selection.source.selectedSetSha256");
  }
  return value;
}

export function validateLock(value, networkPolicy = {}) {
  assertExactKeys(value, ["schemaVersion", "catalog", "entries"], ["schemaVersion", "catalog", "entries"], "lock");
  if (value.schemaVersion !== 1) fail("lock.schemaVersion must be 1");
  validateCatalogMetadata(value.catalog);
  if (!Array.isArray(value.entries)) fail("lock.entries must be an array");
  const ids = new Set();
  const paths = new Set();
  const sourcePaths = new Set();
  value.entries.forEach((entry, entryIndex) => {
    const where = `entries[${entryIndex}]`;
    validateEntryBase(entry, where, (artifact, artifactWhere) => {
      const fields = [
        "kind",
        "sourcePath",
        "url",
        "resolvedUrl",
        "path",
        "bytes",
        "sha256",
        "expectedEntries",
        "entryCountBasis",
        "keySha256",
        "payloadSha256",
        "logicalDigestBasis",
        "logicalObservation",
        "observedEntries",
        "observation",
        "observationError",
        "observer",
      ];
      assertExactKeys(artifact, fields, fields, artifactWhere);
      if (!["mdx", "mdd"].includes(artifact.kind)) fail(`${artifactWhere}.kind must be mdx or mdd`);
      requireString(artifact.sourcePath, `${artifactWhere}.sourcePath`);
      if (/[\t\r\n\0]/.test(artifact.sourcePath)) fail(`${artifactWhere}.sourcePath contains a forbidden control character`);
      requireAcquisitionUrl(artifact.url, `${artifactWhere}.url`, networkPolicy);
      requireAcquisitionUrl(artifact.resolvedUrl, `${artifactWhere}.resolvedUrl`, networkPolicy);
      if (new URL(artifact.url).origin !== new URL(artifact.resolvedUrl).origin) {
        fail(`${artifactWhere}.resolvedUrl must remain on the reviewed URL origin`);
      }
      requireRelativePath(artifact.path, artifact.kind, `${artifactWhere}.path`);
      requireSafeInteger(artifact.bytes, `${artifactWhere}.bytes`, 1);
      if (typeof artifact.sha256 !== "string" || !SHA256.test(artifact.sha256)) {
        fail(`${artifactWhere}.sha256 must be 64 lowercase hexadecimal digits`);
      }
      requireSafeInteger(artifact.expectedEntries, `${artifactWhere}.expectedEntries`);
      if (!["independent", "publisher", "mdictlib-self-observed"].includes(artifact.entryCountBasis)) {
        fail(`${artifactWhere}.entryCountBasis must be independent, publisher, or mdictlib-self-observed`);
      }
      for (const field of ["keySha256", "payloadSha256"]) {
        if (artifact[field] !== null && (typeof artifact[field] !== "string" || !SHA256.test(artifact[field]))) {
          fail(`${artifactWhere}.${field} must be null or 64 lowercase hexadecimal digits`);
        }
      }
      if (artifact.logicalDigestBasis !== null && !["independent", "mdictlib-self-observed"].includes(artifact.logicalDigestBasis)) {
        fail(`${artifactWhere}.logicalDigestBasis must be null, independent, or mdictlib-self-observed`);
      }
      if (artifact.logicalObservation !== null) requireString(artifact.logicalObservation, `${artifactWhere}.logicalObservation`);
      if ((artifact.keySha256 === null) !== (artifact.payloadSha256 === null)) {
        fail(`${artifactWhere} must provide both logical hashes or neither`);
      }
      if ((artifact.keySha256 === null || artifact.payloadSha256 === null) && artifact.logicalDigestBasis !== null) {
        fail(`${artifactWhere}.logicalDigestBasis requires both logical hashes`);
      }
      if (artifact.keySha256 !== null && artifact.payloadSha256 !== null && artifact.logicalDigestBasis === null) {
        fail(`${artifactWhere} logical hashes require logicalDigestBasis`);
      }
      if ((artifact.logicalDigestBasis === null) !== (artifact.logicalObservation === null)) {
        fail(`${artifactWhere}.logicalObservation and logicalDigestBasis must both be present or both be null`);
      }
      if (artifact.observedEntries !== null) requireSafeInteger(artifact.observedEntries, `${artifactWhere}.observedEntries`);
      if (artifact.observation !== null) requireString(artifact.observation, `${artifactWhere}.observation`);
      if (artifact.observationError !== null) requireString(artifact.observationError, `${artifactWhere}.observationError`);
      validateObserver(artifact.observer, `${artifactWhere}.observer`);
      if (artifact.observedEntries !== null) {
        if (artifact.observation === null || artifact.observationError !== null || artifact.observer === null) {
          fail(`${artifactWhere}.observedEntries requires successful observation provenance`);
        }
      }
      if (artifact.entryCountBasis === "mdictlib-self-observed") {
        if (artifact.observedEntries !== artifact.expectedEntries) {
          fail(`${artifactWhere}.expectedEntries must equal observedEntries for mdictlib-self-observed counts`);
        }
      }
      if (!paths.add(artifact.path)) fail(`${artifactWhere}.path duplicates ${artifact.path}`);
      if (!sourcePaths.add(artifact.sourcePath)) fail(`${artifactWhere}.sourcePath duplicates ${artifact.sourcePath}`);
    });
    if (!ids.add(entry.id)) fail(`${where}.id duplicates ${entry.id}`);
  });
  return value;
}

export function approvedArtifacts(lock) {
  return lock.entries
    .filter((entry) => entry.review.status === "approved" && entry.review.testingAllowed)
    .flatMap((entry) => entry.artifacts.map((artifact) => ({ entry, artifact })))
    .sort((left, right) => compareText(left.artifact.path, right.artifact.path));
}

export function manifestText(rows) {
  const lines = [MANIFEST_HEADER];
  for (const { artifact } of [...rows].sort((a, b) => compareText(a.artifact.path, b.artifact.path))) {
    lines.push(
      [
        artifact.path,
        artifact.kind,
        String(artifact.bytes),
        artifact.sha256,
        String(artifact.expectedEntries),
        artifact.keySha256 ?? "",
        artifact.payloadSha256 ?? "",
      ].join("\t"),
    );
  }
  return `${lines.join("\n")}\n`;
}

export function compareText(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

export function resolveCorpusPath(root, relativePath) {
  const resolvedRoot = path.resolve(root);
  const target = path.resolve(resolvedRoot, ...relativePath.split("/"));
  const relation = path.relative(resolvedRoot, target);
  if (relation === "" || relation.startsWith("..") || path.isAbsolute(relation)) {
    fail(`corpus path ${relativePath} resolves outside ${resolvedRoot}`);
  }
  return target;
}

export function downloadStatePaths(destination) {
  const partial = path.join(path.dirname(destination), `.${path.basename(destination)}.part`);
  return {
    partial,
    partialMetadata: `${partial}.json`,
    partialOwnership: `${partial}.lock`,
  };
}

async function existingKind(filePath) {
  try {
    const metadata = await lstat(filePath);
    return metadata.isFile() ? "file" : "other";
  } catch (error) {
    if (error?.code === "ENOENT") return "missing";
    throw error;
  }
}

async function ensureSafeCorpusParent(root, destination, createParents) {
  const resolvedRoot = path.resolve(root);
  const resolvedDestination = path.resolve(destination);
  const relation = path.relative(resolvedRoot, resolvedDestination);
  if (relation === "" || relation.startsWith("..") || path.isAbsolute(relation)) {
    fail(`destination resolves outside corpus root ${resolvedRoot}`);
  }
  if (createParents) await mkdir(resolvedRoot, { recursive: true });
  let rootMetadata;
  try {
    rootMetadata = await lstat(resolvedRoot);
  } catch (error) {
    if (error?.code === "ENOENT") fail(`corpus root ${resolvedRoot} does not exist`);
    throw error;
  }
  if (rootMetadata.isSymbolicLink() || !rootMetadata.isDirectory()) {
    fail(`corpus root ${resolvedRoot} must be a real directory, not a symlink`);
  }
  const canonicalRoot = await realpath(resolvedRoot);
  const parentParts = path.dirname(relation).split(path.sep).filter((part) => part !== "." && part !== "");
  let current = resolvedRoot;
  for (const part of parentParts) {
    current = path.join(current, part);
    if (createParents) {
      try {
        await mkdir(current);
      } catch (error) {
        if (error?.code !== "EEXIST") throw error;
      }
    }
    let metadata;
    try {
      metadata = await lstat(current);
    } catch (error) {
      if (error?.code === "ENOENT") fail(`corpus parent ${current} does not exist`);
      throw error;
    }
    if (metadata.isSymbolicLink() || !metadata.isDirectory()) {
      fail(`corpus parent ${current} must be a real directory, not a symlink`);
    }
    const canonicalCurrent = await realpath(current);
    const canonicalRelation = path.relative(canonicalRoot, canonicalCurrent);
    if (canonicalRelation.startsWith("..") || path.isAbsolute(canonicalRelation)) {
      fail(`corpus parent ${current} resolves outside the corpus root`);
    }
  }
  return resolvedDestination;
}

export async function assertSafeCorpusTarget(root, relativePath, { createParents = false } = {}) {
  const destination = resolveCorpusPath(root, relativePath);
  await ensureSafeCorpusParent(root, destination, createParents);
  return destination;
}

async function openResponse(
  startUrl,
  { signal, maxRedirects, userAgent, requestHeaders = {}, onActivity = () => {}, networkPolicy = {} },
) {
  const initial = requireAcquisitionUrl(startUrl, "download URL", networkPolicy);
  let current = initial;
  for (let redirects = 0; ; redirects += 1) {
    const lookupFunction = await pinnedLookupFor(new URL(current), networkPolicy, signal);
    const response = await requestOnce(current, {
      signal,
      lookupFunction,
      headers: {
        accept: "application/octet-stream",
        "accept-encoding": "identity",
        "user-agent": userAgent,
        ...requestHeaders,
      },
    });
    onActivity();
    if ([301, 302, 303, 307, 308].includes(response.status)) {
      if (redirects >= maxRedirects) fail(`download exceeded ${maxRedirects} redirects`);
      const location = response.headers.get("location");
      await response.body?.cancel();
      if (!location) fail(`redirect from ${current} has no Location header`);
      current = requireSameOriginRedirect(location, current, initial, networkPolicy);
      continue;
    }
    if (!response.ok) {
      await response.body?.cancel();
      fail(`download returned HTTP ${response.status}`);
    }
    const encoding = response.headers.get("content-encoding");
    if (encoding && encoding.toLowerCase() !== "identity") {
      await response.body?.cancel();
      fail(`download of ${current} used unsupported content-encoding ${encoding}`);
    }
    return { response, resolvedUrl: current };
  }
}

export async function downloadAtomic({
  url,
  destination,
  root,
  maxBytes,
  expectedBytes = null,
  expectedSha256 = null,
  expectedResolvedUrl = null,
  timeoutMs,
  deadlineMs = 6 * 60 * 60 * 1000,
  maxRedirects = 5,
  userAgent = "mdictlib-corpus-tool/0.1",
  networkPolicy = {},
}) {
  await ensureSafeCorpusParent(root, destination, true);
  const {
    partial: temporary,
    partialMetadata: partialMetadataPath,
    partialOwnership: ownershipPath,
  } = downloadStatePaths(destination);
  const controller = new AbortController();
  let timer;
  const deadlineTimer = setTimeout(
    () => controller.abort(new Error(`download attempt exceeded the ${deadlineMs} ms absolute deadline`)),
    deadlineMs,
  );
  const resetInactivityTimer = () => {
    clearTimeout(timer);
    timer = setTimeout(
      () => controller.abort(new Error(`download had no network activity for ${timeoutMs} ms`)),
      timeoutMs,
    );
  };
  let handle;
  let ownershipHandle;
  let activeResponse;
  let ownsPartial = false;
  let keepPartial = false;
  let bytes = 0;
  let partialBytes = 0;
  try {
    try {
      ownershipHandle = await open(
        ownershipPath,
        constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | NOFOLLOW,
        0o600,
      );
      ownsPartial = true;
      await ownershipHandle.writeFile(`pid=${process.pid}\n`, "utf8");
      await ownershipHandle.sync();
    } catch (error) {
      if (error?.code === "EEXIST") fail("partial download is already owned by another process");
      throw error;
    }
    if ((await existingKind(destination)) !== "missing") {
      fail(`refusing to overwrite existing corpus path ${destination}`);
    }
    const canonicalUrl = requireAcquisitionUrl(url, "download URL", networkPolicy);
    const canonicalExpectedResolvedUrl = expectedResolvedUrl === null
      ? null
      : requireAcquisitionUrl(expectedResolvedUrl, "expected resolved URL", networkPolicy);
    const installVerifiedPartial = async ({ bytes, sha256, resolvedUrl }) => {
      if (!handle) fail("partial download handle is not open");
      await handle.sync();
      const identity = requirePartialIdentity(
        await handle.stat({ bigint: true }),
        bytes,
        "completed partial download",
      );
      const physicalHash = createHash("sha256");
      const physicalBytes = await updateHashFromHandle(handle, physicalHash, identity, controller.signal);
      if (physicalBytes !== bytes || physicalHash.digest("hex") !== sha256) {
        keepPartial = false;
        fail("completed partial bytes differ from the streamed digest");
      }
      await ensureSafeCorpusParent(root, destination, false);
      if ((await existingKind(destination)) !== "missing") {
        fail(`corpus path appeared during download: ${destination}`);
      }
      let installed = false;
      let installedHandle;
      try {
        await link(temporary, destination);
        installed = true;
        installedHandle = await open(destination, constants.O_RDONLY | NOFOLLOW);
        const installedMetadata = await installedHandle.stat({ bigint: true });
        const installedIdentity = {
          dev: identity.dev,
          ino: identity.ino,
          nlink: 2n,
          size: identity.size,
        };
        if (
          !installedMetadata.isFile() ||
          installedMetadata.dev !== installedIdentity.dev ||
          installedMetadata.ino !== installedIdentity.ino ||
          installedMetadata.nlink !== installedIdentity.nlink ||
          installedMetadata.size !== installedIdentity.size
        ) {
          fail("installed corpus path is not the verified partial inode");
        }
        const installedHash = createHash("sha256");
        const installedBytes = await updateHashFromHandle(
          installedHandle,
          installedHash,
          installedIdentity,
          controller.signal,
        );
        if (installedBytes !== bytes || installedHash.digest("hex") !== sha256) {
          fail("installed corpus path differs from the verified partial bytes");
        }
        await installedHandle.close();
        installedHandle = undefined;
        await handle.close();
        handle = undefined;
        await rm(temporary);
        await rm(partialMetadataPath, { force: true });
        keepPartial = false;
        return { bytes, sha256, resolvedUrl };
      } catch (error) {
        await installedHandle?.close();
        if (installed) await rm(destination, { force: true });
        throw error;
      }
    };
    let partialMetadata = null;
    partialBytes = 0;
    const partialKind = await existingKind(temporary);
    const metadataKind = await existingKind(partialMetadataPath);
    if (partialKind !== "missing" || metadataKind !== "missing") {
      if (partialKind !== "file" || metadataKind !== "file") {
        fail("partial download state must contain only regular files, not symlinks");
      } else {
        try {
          partialMetadata = await readJson(partialMetadataPath);
          assertExactKeys(
            partialMetadata,
            ["schemaVersion", "url", "resolvedUrl", "etag", "lastModified", "totalBytes", "expectedBytes", "expectedSha256"],
            ["schemaVersion", "url", "resolvedUrl", "etag", "lastModified", "totalBytes", "expectedBytes", "expectedSha256"],
            "partial metadata",
          );
          const metadata = await stat(temporary);
          partialBytes = metadata.size;
          const compatible =
            partialMetadata.schemaVersion === 1 &&
            partialMetadata.url === canonicalUrl &&
            partialMetadata.expectedBytes === expectedBytes &&
            partialMetadata.expectedSha256 === expectedSha256 &&
            (canonicalExpectedResolvedUrl === null || partialMetadata.resolvedUrl === canonicalExpectedResolvedUrl) &&
            (partialMetadata.totalBytes === null ||
              (Number.isSafeInteger(partialMetadata.totalBytes) && partialMetadata.totalBytes >= partialBytes)) &&
            partialBytes <= maxBytes &&
            (expectedBytes === null || partialBytes <= expectedBytes);
          if (!compatible) fail("partial metadata does not match this locked download");
        } catch {
          await rm(temporary, { force: true });
          await rm(partialMetadataPath, { force: true });
          partialMetadata = null;
          partialBytes = 0;
        }
      }
    }

    let prefixHash = createHash("sha256");
    if (partialBytes > 0) {
      handle = await open(temporary, constants.O_RDWR | constants.O_APPEND | NOFOLLOW);
      const prefixIdentity = requirePartialIdentity(
        await handle.stat({ bigint: true }),
        partialBytes,
      );
      const hashedBytes = await updateHashFromHandle(
        handle,
        prefixHash,
        prefixIdentity,
        controller.signal,
      );
      if (hashedBytes !== partialBytes) fail("partial download changed while it was being hashed");
      if (expectedBytes !== null && partialBytes === expectedBytes) {
        const completeHash = prefixHash.copy().digest("hex");
        if (completeHash === expectedSha256) {
          return installVerifiedPartial({
            bytes: partialBytes,
            sha256: completeHash,
            resolvedUrl: partialMetadata.resolvedUrl,
          });
        }
        await handle.close();
        handle = undefined;
        await rm(temporary, { force: true });
        await rm(partialMetadataPath, { force: true });
        partialMetadata = null;
        partialBytes = 0;
        prefixHash = createHash("sha256");
      }
      if (partialBytes > 0 && partialMetadata.etag === null && partialMetadata.lastModified === null) {
        await handle.close();
        handle = undefined;
        await rm(temporary, { force: true });
        await rm(partialMetadataPath, { force: true });
        partialMetadata = null;
        partialBytes = 0;
        prefixHash = createHash("sha256");
      }
    }

    const request = async (offset, metadata) => {
      const requestHeaders = {};
      if (offset > 0) {
        requestHeaders.range = `bytes=${offset}-`;
        const validator = metadata.etag ?? metadata.lastModified;
        if (validator) requestHeaders["if-range"] = validator;
      }
      return openResponse(canonicalUrl, {
        signal: controller.signal,
        maxRedirects,
        userAgent,
        requestHeaders,
        onActivity: resetInactivityTimer,
        networkPolicy,
      });
    };

    bytes = partialBytes;
    keepPartial = partialBytes > 0;
    resetInactivityTimer();
    let opened = await request(partialBytes, partialMetadata);
    activeResponse = opened.response;
    if (partialBytes > 0 && opened.response.status === 200) {
      await opened.response.body?.cancel();
      activeResponse = undefined;
      await handle?.close();
      handle = undefined;
      await rm(temporary, { force: true });
      await rm(partialMetadataPath, { force: true });
      partialMetadata = null;
      partialBytes = 0;
      prefixHash = createHash("sha256");
      keepPartial = false;
      opened = await request(0, null);
      activeResponse = opened.response;
    }
    const { response, resolvedUrl } = opened;
    keepPartial = false;
    if (canonicalExpectedResolvedUrl !== null && resolvedUrl !== canonicalExpectedResolvedUrl) {
      await response.body?.cancel();
      fail("resolved URL changed from the reviewed lock");
    }
    if (partialMetadata && resolvedUrl !== partialMetadata.resolvedUrl) {
      await response.body?.cancel();
      fail(`resolved URL changed while resuming: got ${resolvedUrl}; partial is from ${partialMetadata.resolvedUrl}`);
    }
    const rawLength = response.headers.get("content-length");
    let contentLength = null;
    if (rawLength !== null) {
      if (!/^[0-9]+$/.test(rawLength)) fail(`invalid Content-Length ${JSON.stringify(rawLength)}`);
      contentLength = Number(rawLength);
      if (!Number.isSafeInteger(contentLength)) fail("Content-Length exceeds JavaScript's safe integer range");
    }
    let totalBytes = contentLength;
    if (partialBytes > 0) {
      if (response.status !== 206) fail(`range resume returned HTTP ${response.status}, expected 206`);
      const contentRange = response.headers.get("content-range");
      const match = contentRange?.match(/^bytes ([0-9]+)-([0-9]+)\/([0-9]+)$/);
      if (!match) fail(`range response has invalid Content-Range ${JSON.stringify(contentRange)}`);
      const start = Number(match[1]);
      const end = Number(match[2]);
      totalBytes = Number(match[3]);
      if (![start, end, totalBytes].every(Number.isSafeInteger) || start !== partialBytes || end < start || totalBytes <= end) {
        fail(`range response does not continue the saved ${partialBytes}-byte prefix`);
      }
      if (contentLength !== null && contentLength !== end - start + 1) {
        fail("range Content-Length does not match Content-Range");
      }
      const responseEtag = response.headers.get("etag");
      const responseLastModified = response.headers.get("last-modified");
      if (partialMetadata.etag && responseEtag !== partialMetadata.etag) {
        fail("range response ETag is missing or differs from the saved partial");
      }
      if (!partialMetadata.etag && partialMetadata.lastModified && responseLastModified !== partialMetadata.lastModified) {
        fail("range response Last-Modified is missing or differs from the saved partial");
      }
    } else if (response.status !== 200 && response.status !== 206) {
      fail(`initial download returned unexpected HTTP ${response.status}`);
    } else if (response.status === 206) {
      const contentRange = response.headers.get("content-range");
      const match = contentRange?.match(/^bytes 0-([0-9]+)\/([0-9]+)$/);
      if (!match) fail(`initial range response has invalid Content-Range ${JSON.stringify(contentRange)}`);
      totalBytes = Number(match[2]);
    }
    if (totalBytes !== null) {
      if (!Number.isSafeInteger(totalBytes)) fail("response size exceeds JavaScript's safe integer range");
      if (totalBytes > maxBytes) fail(`response size ${totalBytes} exceeds ${maxBytes}-byte file limit`);
      if (expectedBytes !== null && totalBytes !== expectedBytes) {
        fail(`response size ${totalBytes} differs from locked size ${expectedBytes}`);
      }
    }

    const newPartialMetadata = {
      schemaVersion: 1,
      url: canonicalUrl,
      resolvedUrl,
      etag: response.headers.get("etag"),
      lastModified: response.headers.get("last-modified"),
      totalBytes,
      expectedBytes,
      expectedSha256,
    };
    if (partialMetadata === null) {
      await writeTextAtomic(partialMetadataPath, stableJson(newPartialMetadata));
      keepPartial = true;
    }
    await ensureSafeCorpusParent(root, destination, false);
    const openFlags = partialMetadata === null
      ? constants.O_RDWR | constants.O_CREAT | constants.O_EXCL | NOFOLLOW
      : constants.O_RDWR | constants.O_APPEND | NOFOLLOW;
    if (!handle) handle = await open(temporary, openFlags, 0o600);
    requirePartialIdentity(
      await handle.stat({ bigint: true }),
      partialBytes,
      "opened partial download",
    );
    await ensureSafeCorpusParent(root, destination, false);
    const hash = prefixHash;
    bytes = partialBytes;
    keepPartial = true;
    if (response.body === null) fail("successful download has no response body");
    for await (const rawChunk of response.body) {
      resetInactivityTimer();
      const chunk = Buffer.from(rawChunk);
      const next = bytes + chunk.length;
      if (!Number.isSafeInteger(next) || next > maxBytes) {
        keepPartial = false;
        fail(`download exceeds ${maxBytes}-byte file limit`);
      }
      if (expectedBytes !== null && next > expectedBytes) {
        keepPartial = false;
        fail(`download exceeds locked size ${expectedBytes}`);
      }
      let offset = 0;
      while (offset < chunk.length) {
        const { bytesWritten } = await handle.write(chunk, offset, chunk.length - offset);
        if (bytesWritten === 0) fail("short write while saving download");
        offset += bytesWritten;
      }
      hash.update(chunk);
      bytes = next;
    }
    activeResponse = undefined;
    const receivedThisRequest = bytes - partialBytes;
    if (contentLength !== null && receivedThisRequest !== contentLength) {
      fail(`truncated download: received ${receivedThisRequest} response bytes; Content-Length declared ${contentLength}`);
    }
    if (totalBytes !== null && bytes !== totalBytes) {
      fail(`truncated download: received ${bytes} total bytes; response declared ${totalBytes}`);
    }
    if (expectedBytes !== null && bytes !== expectedBytes) {
      fail(`downloaded ${bytes} bytes; locked size is ${expectedBytes}`);
    }
    const sha256 = hash.digest("hex");
    if (expectedSha256 !== null && sha256 !== expectedSha256) {
      keepPartial = false;
      fail(`downloaded SHA-256 ${sha256}; locked SHA-256 is ${expectedSha256}`);
    }
    return await installVerifiedPartial({ bytes, sha256, resolvedUrl });
  } catch (error) {
    controller.abort(error instanceof Error ? error : new Error("download attempt failed"));
    try {
      await activeResponse?.body?.cancel();
    } catch {
      // Preserve the original download error.
    }
    activeResponse = undefined;
    try {
      if (keepPartial && handle) await handle.sync();
    } catch {
      // Preserve the original download error.
    }
    try {
      await handle?.close();
    } catch {
      // Preserve the original download error.
    }
    if (ownsPartial && !keepPartial) {
      await rm(temporary, { force: true });
      await rm(partialMetadataPath, { force: true });
    }
    const normalized = error instanceof Error ? error : new Error(String(error));
    if (keepPartial && bytes > partialBytes && !/truncated|terminated|aborted|ECONNRESET|network activity/i.test(normalized.message)) {
      throw new Error(`truncated download interrupted after ${bytes} bytes: ${normalized.message}`, { cause: normalized });
    }
    throw normalized;
  } finally {
    clearTimeout(timer);
    clearTimeout(deadlineTimer);
    try {
      await ownershipHandle?.close();
    } finally {
      if (ownsPartial) await rm(ownershipPath, { force: true });
    }
  }
}

export async function verifyArtifact(root, artifact) {
  const target = await assertSafeCorpusTarget(root, artifact.path);
  const kind = await existingKind(target);
  if (kind !== "file") fail(`${artifact.path} is ${kind === "missing" ? "missing" : "not a regular file"}`);
  const actual = await sha256File(target);
  await assertSafeCorpusTarget(root, artifact.path);
  if (actual.bytes !== artifact.bytes) fail(`${artifact.path} has ${actual.bytes} bytes; expected ${artifact.bytes}`);
  if (actual.sha256 !== artifact.sha256) fail(`${artifact.path} has SHA-256 ${actual.sha256}; expected ${artifact.sha256}`);
  return target;
}

export async function mapBounded(items, concurrency, worker) {
  const results = new Array(items.length);
  let cursor = 0;
  async function run() {
    while (true) {
      const index = cursor;
      cursor += 1;
      if (index >= items.length) return;
      results[index] = await worker(items[index], index);
    }
  }
  await Promise.all(Array.from({ length: Math.min(concurrency, items.length) }, run));
  return results;
}

export async function assertDistinctPaths(namedPaths) {
  const seenLexical = new Map();
  const seenCanonical = new Map();
  const seenInodes = new Map();
  for (const [name, value] of Object.entries(namedPaths)) {
    if (value === null || value === undefined) continue;
    const absolute = path.resolve(value);
    const lexicalKey = ["darwin", "win32"].includes(process.platform)
      ? absolute.normalize("NFC").toLowerCase()
      : absolute;
    const priorLexical = seenLexical.get(lexicalKey);
    if (priorLexical) fail(`${name} must not alias ${priorLexical}`);
    seenLexical.set(lexicalKey, name);
    let existingAncestor = absolute;
    const missingSuffix = [];
    while (true) {
      try {
        await lstat(existingAncestor);
        break;
      } catch (error) {
        if (error?.code !== "ENOENT") throw error;
        const parent = path.dirname(existingAncestor);
        if (parent === existingAncestor) throw error;
        missingSuffix.unshift(path.basename(existingAncestor));
        existingAncestor = parent;
      }
    }
    const canonicalAncestor = await realpath(existingAncestor);
    const canonical = path.resolve(canonicalAncestor, ...missingSuffix);
    const canonicalKey = ["darwin", "win32"].includes(process.platform)
      ? canonical.normalize("NFC").toLowerCase()
      : canonical;
    const priorCanonical = seenCanonical.get(canonicalKey);
    if (priorCanonical) fail(`${name} must not alias ${priorCanonical} through filesystem links`);
    seenCanonical.set(canonicalKey, name);
    try {
      const metadata = await lstat(absolute, { bigint: true });
      const inodeKey = `${metadata.dev}:${metadata.ino}`;
      const priorInode = seenInodes.get(inodeKey);
      if (priorInode) fail(`${name} must not alias ${priorInode}`);
      seenInodes.set(inodeKey, name);
    } catch (error) {
      if (error?.code !== "ENOENT") throw error;
    }
  }
}

export function parseOptions(argv, definitions) {
  const result = {};
  for (let index = 0; index < argv.length; index += 1) {
    const name = argv[index];
    const definition = definitions[name];
    if (!definition) fail(`unknown option ${name}`);
    if (definition === "boolean") {
      result[name.slice(2)] = true;
      continue;
    }
    const value = argv[++index];
    if (value === undefined) fail(`${name} requires a value`);
    result[name.slice(2)] = value;
  }
  return result;
}

export function positiveOption(value, fallback, name, { allowZero = false } = {}) {
  if (value === undefined) return fallback;
  if (!/^[0-9]+$/.test(value)) fail(`${name} must be an unsigned integer`);
  const parsed = Number(value);
  const minimum = allowZero ? 0 : 1;
  if (!Number.isSafeInteger(parsed) || parsed < minimum) fail(`${name} must be a safe integer >= ${minimum}`);
  return parsed;
}
