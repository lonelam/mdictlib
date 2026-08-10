#!/usr/bin/env node

import { createHash } from "node:crypto";
import process from "node:process";
import { pathToFileURL } from "node:url";
import {
  assertExactKeys,
  assertDistinctPaths,
  compareText,
  fail,
  parseOptions,
  readJson,
  requireAcquisitionUrl,
  requireRelativePath,
  requireSafeInteger,
  requireString,
  sha256File,
  stableJson,
  sourceRowSetSha256,
  validateSelection,
  writeTextAtomic,
} from "./lib.mjs";

function hashUrl(url) {
  return createHash("sha256").update(url).digest("hex");
}

export function selectInventory(inventory, {
  type,
  inventorySha256,
  reviewedBy,
  reviewedAt,
  notes,
  networkPolicy = {},
}) {
  const topFields = [
    "schemaVersion",
    "root",
    "snapshotAt",
    "pageCount",
    "fileCount",
    "advertisedBytes",
    "files",
  ];
  assertExactKeys(inventory, topFields, topFields, "inventory");
  if (inventory.schemaVersion !== 1) fail("inventory.schemaVersion must be 1");
  if (typeof inventorySha256 !== "string" || !/^[0-9a-f]{64}$/.test(inventorySha256)) {
    fail("inventorySha256 must be 64 lowercase hexadecimal digits");
  }
  const root = requireAcquisitionUrl(inventory.root, "inventory.root", networkPolicy);
  const rootUrl = new URL(root);
  requireString(inventory.snapshotAt, "inventory.snapshotAt");
  if (!Number.isFinite(Date.parse(inventory.snapshotAt))) fail("inventory.snapshotAt is not an ISO date-time");
  requireSafeInteger(inventory.pageCount, "inventory.pageCount");
  requireSafeInteger(inventory.fileCount, "inventory.fileCount");
  requireSafeInteger(inventory.advertisedBytes, "inventory.advertisedBytes");
  if (!Array.isArray(inventory.files)) fail("inventory.files must be an array");
  if (inventory.fileCount !== inventory.files.length) fail("inventory.fileCount does not match files.length");
  const allPaths = new Set();
  const selected = [];
  let calculatedBytes = 0;
  inventory.files.forEach((file, index) => {
    const where = `inventory.files[${index}]`;
    const fields = ["path", "type", "bytes", "url", "parent"];
    assertExactKeys(file, fields, fields, where);
    requireString(file.path, `${where}.path`);
    requireString(file.type, `${where}.type`);
    requireSafeInteger(file.bytes, `${where}.bytes`);
    const fileUrl = new URL(requireAcquisitionUrl(file.url, `${where}.url`, networkPolicy));
    if (fileUrl.origin !== rootUrl.origin || !fileUrl.pathname.startsWith(rootUrl.pathname)) {
      fail(`${where}.url must stay on the inventory origin and beneath its root path`);
    }
    if (typeof file.parent !== "string") fail(`${where}.parent must be a string`);
    if (!allPaths.add(file.path)) fail(`${where}.path duplicates ${file.path}`);
    calculatedBytes += file.bytes;
    if (!Number.isSafeInteger(calculatedBytes)) fail("inventory byte sum exceeds the safe integer range");
    if (file.type === type) selected.push(file);
  });
  if (calculatedBytes !== inventory.advertisedBytes) {
    fail(`inventory.advertisedBytes is ${inventory.advertisedBytes}; file sum is ${calculatedBytes}`);
  }
  if (selected.length === 0) fail(`inventory contains no ${type} files`);

  const review = {
    license: "unverified",
    licenseUrl: null,
    notes,
    redistributionAllowed: false,
    reviewedAt,
    reviewedBy,
    status: "approved",
    testingAllowed: true,
  };
  const selectedAdvertisedBytes = selected.reduce((sum, file) => sum + file.bytes, 0);
  const selectedFacts = selected.map((file) => ({
    advertisedBytes: file.bytes,
    kind: type,
    sourcePath: file.path,
    url: new URL(file.url).toString(),
  }));
  const result = {
    catalog: {
      name: `mdict.org ${type.toUpperCase()} local validation selection`,
      scope:
        `All ${selected.length} direct .${type} rows in the deterministic ${inventory.snapshotAt} inventory snapshot at ${root}. ` +
        "Approval is for private local parser testing only and makes no redistribution or exhaustiveness claim beyond that snapshot.",
    },
    entries: selected
      .map((file) => {
        const canonicalUrl = new URL(file.url).toString();
        const urlHash = hashUrl(canonicalUrl);
        const localPath = `mdict-org/${type}/${urlHash.slice(0, 32)}.${type}`;
        requireRelativePath(localPath, type, `local path for ${JSON.stringify(file.path)}`);
        return {
          artifacts: [{
            advertisedBytes: file.bytes,
            kind: type,
            path: localPath,
            sourcePath: file.path,
            url: canonicalUrl,
          }],
          id: `mdict-org-${urlHash.slice(0, 24)}`,
          infoUrl: new URL("./", file.url).toString(),
          review: { ...review },
          title: file.path,
        };
      })
      .sort((left, right) => compareText(left.title, right.title)),
    schemaVersion: 1,
    source: {
      inventorySha256,
      kind: "mdict-index-inventory-v1",
      root,
      selectedAdvertisedBytes,
      selectedCount: selected.length,
      selectedSetSha256: sourceRowSetSha256(selectedFacts),
      selectedType: type,
      snapshotAt: inventory.snapshotAt,
    },
  };
  return validateSelection(result, networkPolicy);
}

export function validateSelectionAgainstInventory(
  selection,
  inventory,
  inventorySha256,
  networkPolicy = {},
) {
  validateSelection(selection, networkPolicy);
  if (selection.source.inventorySha256 !== inventorySha256) {
    fail("selection source inventorySha256 does not match the supplied inventory bytes");
  }
  const first = selection.entries[0];
  if (!first) fail("selection has no entries");
  const expected = selectInventory(inventory, {
    inventorySha256,
    networkPolicy,
    notes: first.review.notes,
    reviewedAt: first.review.reviewedAt,
    reviewedBy: first.review.reviewedBy,
    type: selection.source.selectedType,
  });
  if (stableJson(expected.source) !== stableJson(selection.source)) {
    fail("selection source facts do not match the supplied inventory");
  }
  const expectedBySourcePath = new Map(expected.entries.map((entry) => [entry.artifacts[0].sourcePath, entry]));
  if (selection.entries.length !== expected.entries.length) {
    fail(`selection has ${selection.entries.length} entries; inventory requires ${expected.entries.length}`);
  }
  for (const entry of selection.entries) {
    const artifact = entry.artifacts[0];
    const expectedEntry = expectedBySourcePath.get(artifact.sourcePath);
    if (!expectedEntry) fail(`selection contains source path absent from inventory: ${artifact.sourcePath}`);
    if (
      entry.id !== expectedEntry.id ||
      entry.title !== expectedEntry.title ||
      entry.infoUrl !== expectedEntry.infoUrl ||
      stableJson(artifact) !== stableJson(expectedEntry.artifacts[0])
    ) {
      fail(`selection row does not match inventory source path ${artifact.sourcePath}`);
    }
  }
  return selection;
}

export async function main(argv = process.argv.slice(2)) {
  const options = parseOptions(argv, {
    "--input": "string",
    "--type": "string",
    "--output": "string",
    "--reviewed-by": "string",
    "--reviewed-at": "string",
    "--notes": "string",
    "--approve-local-testing": "boolean",
  });
  if (!options.input || !options.output || !options.type) {
    fail("usage: select-inventory.mjs --input <inventory.json> --type <mdx|mdd> --output <selection.json> --reviewed-by <reviewer> --approve-local-testing");
  }
  if (!options["approve-local-testing"] || !options["reviewed-by"]) {
    fail("selection requires both --approve-local-testing and --reviewed-by; no approval is inferred from source availability");
  }
  const type = options.type.toLowerCase();
  if (!["mdx", "mdd"].includes(type)) fail("--type must be mdx or mdd");
  const reviewedAt = options["reviewed-at"] ?? new Date().toISOString();
  if (!Number.isFinite(Date.parse(reviewedAt))) fail("--reviewed-at must be an ISO date-time");
  const notes = options.notes ??
    "Maintainer requested private local parser validation; public source availability is not evidence of redistribution permission.";
  await assertDistinctPaths({ input: options.input, output: options.output });
  const [inventory, identityBefore] = await Promise.all([readJson(options.input), sha256File(options.input)]);
  const identityAfter = await sha256File(options.input);
  if (identityBefore.bytes !== identityAfter.bytes || identityBefore.sha256 !== identityAfter.sha256) {
    fail("inventory changed while it was being read");
  }
  const inventorySha256 = identityBefore.sha256;
  const result = selectInventory(inventory, {
    type,
    inventorySha256,
    reviewedBy: options["reviewed-by"],
    reviewedAt,
    notes,
  });
  await writeTextAtomic(options.output, stableJson(result));
  process.stdout.write(
    `Selected ${result.entries.length} .${type} artifacts for explicitly reviewed private local testing. ` +
      "Redistribution remains false.\n",
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`select-inventory: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
