#!/usr/bin/env node

import { lstat } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import process from "node:process";
import {
  DEFAULT_MAX_FILE_BYTES,
  DEFAULT_MAX_TOTAL_BYTES,
  MANIFEST_NAME,
  approvedArtifacts,
  assertDistinctPaths,
  downloadStatePaths,
  downloadAtomic,
  fail,
  manifestText,
  mapBounded,
  parseOptions,
  positiveOption,
  readJson,
  resolveCorpusPath,
  validateLock,
  verifyArtifact,
  writeTextAtomic,
} from "./lib.mjs";

async function exists(filePath) {
  try {
    await lstat(filePath);
    return true;
  } catch (error) {
    if (error?.code === "ENOENT") return false;
    throw error;
  }
}

async function downloadWithRetries(row, settings) {
  const { artifact } = row;
  const destination = resolveCorpusPath(settings.root, artifact.path);
  if (await exists(destination)) {
    await verifyArtifact(settings.root, artifact);
    process.stdout.write(`reuse ${artifact.path}\n`);
    return "reused";
  }
  let lastError;
  for (let attempt = 0; attempt <= settings.retries; attempt += 1) {
    try {
      await downloadAtomic({
        url: artifact.url,
        destination,
        root: settings.root,
        maxBytes: Math.min(settings.maxFileBytes, artifact.bytes),
        expectedBytes: artifact.bytes,
        expectedSha256: artifact.sha256,
        expectedResolvedUrl: artifact.resolvedUrl,
        timeoutMs: settings.timeoutMs,
        deadlineMs: settings.deadlineMs,
        networkPolicy: settings.networkPolicy,
      });
      process.stdout.write(`download ${artifact.path}\n`);
      return "downloaded";
    } catch (error) {
      lastError = error;
      if (attempt < settings.retries) {
        process.stderr.write(
          `retry ${artifact.path} (${attempt + 1}/${settings.retries}): ${error instanceof Error ? error.message : String(error)}\n`,
        );
      }
    }
  }
  throw new Error(
    `${artifact.path}: ${lastError instanceof Error ? lastError.message : String(lastError)}`,
    { cause: lastError },
  );
}

export async function syncCorpus({
  catalogPath,
  root,
  concurrency = 2,
  retries = 2,
  timeoutMs = 120_000,
  deadlineMs = 6 * 60 * 60 * 1000,
  maxFileBytes = DEFAULT_MAX_FILE_BYTES,
  maxTotalBytes = DEFAULT_MAX_TOTAL_BYTES,
  networkPolicy = {},
}) {
  const lock = validateLock(await readJson(catalogPath), networkPolicy);
  const rows = approvedArtifacts(lock);
  if (rows.length === 0) fail("reviewed lock has no approved local-testing dictionary artifacts");
  const totalBytes = rows.reduce((sum, row) => {
    const next = sum + row.artifact.bytes;
    if (!Number.isSafeInteger(next)) fail("locked corpus byte total exceeds JavaScript's safe integer range");
    return next;
  }, 0);
  if (totalBytes > maxTotalBytes) {
    fail(`locked corpus is ${totalBytes} bytes, exceeding the ${maxTotalBytes}-byte total limit`);
  }
  for (const row of rows) {
    if (row.artifact.bytes > maxFileBytes) {
      fail(`${row.artifact.path} is ${row.artifact.bytes} bytes, exceeding the ${maxFileBytes}-byte file limit`);
    }
  }

  const manifestPath = path.join(root, MANIFEST_NAME);
  await assertDistinctPaths({
    catalog: catalogPath,
    manifest: manifestPath,
    ...Object.fromEntries(rows.flatMap(({ artifact }, index) => {
      const destination = resolveCorpusPath(root, artifact.path);
      const state = downloadStatePaths(destination);
      return [
        [`artifact-${index}`, destination],
        [`partial-${index}`, state.partial],
        [`partial-metadata-${index}`, state.partialMetadata],
        [`partial-owner-${index}`, state.partialOwnership],
      ];
    })),
  });

  const results = await mapBounded(rows, concurrency, (row) =>
    downloadWithRetries(row, { root, retries, timeoutMs, deadlineMs, maxFileBytes, networkPolicy }),
  );
  for (const { artifact } of rows) await verifyArtifact(root, artifact);
  await writeTextAtomic(manifestPath, manifestText(rows));
  return {
    artifacts: rows.length,
    bytes: totalBytes,
    downloaded: results.filter((result) => result === "downloaded").length,
    manifestPath,
    reused: results.filter((result) => result === "reused").length,
  };
}

export async function main(argv = process.argv.slice(2), internal = {}) {
  const options = parseOptions(argv, {
    "--catalog": "string",
    "--root": "string",
    "--concurrency": "string",
    "--retries": "string",
    "--timeout-ms": "string",
    "--deadline-ms": "string",
    "--max-file-bytes": "string",
    "--max-total-bytes": "string",
  });
  const result = await syncCorpus({
    catalogPath: options.catalog ?? path.join("corpus", "catalog.lock.json"),
    root: options.root ?? ".corpus",
    concurrency: positiveOption(options.concurrency, 2, "--concurrency"),
    retries: positiveOption(options.retries, 2, "--retries", { allowZero: true }),
    timeoutMs: positiveOption(options["timeout-ms"], 120_000, "--timeout-ms"),
    deadlineMs: positiveOption(options["deadline-ms"], 6 * 60 * 60 * 1000, "--deadline-ms"),
    maxFileBytes: positiveOption(options["max-file-bytes"], DEFAULT_MAX_FILE_BYTES, "--max-file-bytes"),
    maxTotalBytes: positiveOption(options["max-total-bytes"], DEFAULT_MAX_TOTAL_BYTES, "--max-total-bytes"),
    networkPolicy: internal.networkPolicy ?? {},
  });
  process.stdout.write(
    `Corpus ready: ${result.artifacts} artifacts, ${result.bytes} bytes; ` +
      `${result.downloaded} downloaded, ${result.reused} reused.\nManifest: ${result.manifestPath}\n`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`sync-corpus: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
