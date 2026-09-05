#!/usr/bin/env node

import { chmod, copyFile, lstat, mkdtemp, readFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";
import {
  DEFAULT_MAX_FILE_BYTES,
  DEFAULT_MAX_TOTAL_BYTES,
  assertDistinctPaths,
  assertSafeCorpusTarget,
  downloadStatePaths,
  downloadAtomic,
  fail,
  mapBounded,
  parseOptions,
  positiveOption,
  readJson,
  requireAcquisitionUrl,
  resolveCorpusPath,
  sanitizeDiagnostic,
  sha256File,
  sha256Text,
  stableJson,
  selectionArtifactSetSha256,
  validateSelection,
  verifyArtifact,
  writeTextAtomic,
} from "./lib.mjs";
import { validateSelectionAgainstInventory } from "./select-inventory.mjs";

async function exists(filePath) {
  try {
    const metadata = await lstat(filePath);
    return metadata.isFile();
  } catch (error) {
    if (error?.code === "ENOENT") return false;
    throw error;
  }
}

async function acquire(artifact, settings) {
  const destination = await assertSafeCorpusTarget(settings.root, artifact.path, { createParents: true });
  const journalPath = `${destination}.mdictlib-lock.json`;
  if (await exists(destination)) {
    if (!(await exists(journalPath))) {
      fail(`refusing unrecorded existing bootstrap file ${artifact.path}`);
    }
    let journal = await readJson(journalPath);
    const legacyFields = [
      "advertisedBytes", "bytes", "kind", "path", "resolvedUrl", "schemaVersion", "sha256", "url",
    ].sort();
    const currentFields = [
      ...legacyFields,
      "inventorySha256",
      "selectionSha256",
      "sourcePath",
    ].sort();
    const actualFields = Object.keys(journal).sort();
    const legacy = actualFields.join("\0") === legacyFields.join("\0");
    if (!legacy && actualFields.join("\0") !== currentFields.join("\0")) {
      fail(`bootstrap journal for ${artifact.path} has an unknown shape`);
    }
    if (
      journal.schemaVersion !== 1 ||
      journal.kind !== artifact.kind ||
      journal.url !== new URL(artifact.url).toString() ||
      journal.path !== artifact.path ||
      journal.advertisedBytes !== artifact.advertisedBytes ||
      journal.bytes !== artifact.advertisedBytes ||
      !Number.isSafeInteger(journal.bytes) ||
      journal.bytes < 1 ||
      typeof journal.sha256 !== "string" ||
      !/^[0-9a-f]{64}$/.test(journal.sha256)
    ) {
      fail(`bootstrap journal for ${artifact.path} does not match the selection`);
    }
    requireAcquisitionUrl(journal.url, `journal URL for ${artifact.path}`, settings.networkPolicy);
    requireAcquisitionUrl(journal.resolvedUrl, `journal resolved URL for ${artifact.path}`, settings.networkPolicy);
    if (new URL(journal.url).origin !== new URL(journal.resolvedUrl).origin) {
      fail(`bootstrap journal for ${artifact.path} crossed origins`);
    }
    if (
      !legacy &&
      (journal.sourcePath !== artifact.sourcePath ||
        journal.selectionSha256 !== settings.selectionBinding.selectionSha256 ||
        journal.inventorySha256 !== settings.selectionBinding.source.inventorySha256)
    ) {
      fail(`bootstrap journal provenance for ${artifact.path} does not match the bound selection`);
    }
    const actual = await sha256File(destination);
    await assertSafeCorpusTarget(settings.root, artifact.path);
    if (actual.bytes !== journal.bytes || actual.sha256 !== journal.sha256) {
      fail(`bootstrap file ${artifact.path} differs from its journal`);
    }
    if (legacy) {
      journal = {
        ...journal,
        inventorySha256: settings.selectionBinding.source.inventorySha256,
        selectionSha256: settings.selectionBinding.selectionSha256,
        sourcePath: artifact.sourcePath,
      };
      await assertSafeCorpusTarget(settings.root, artifact.path);
      await writeTextAtomic(journalPath, stableJson(journal));
      process.stdout.write(`upgraded legacy journal ${artifact.path}\n`);
    }
    settings.budget.used += actual.bytes;
    if (!Number.isSafeInteger(settings.budget.used) || settings.budget.used > settings.maxTotalBytes) {
      fail(`bootstrap corpus exceeds the ${settings.maxTotalBytes}-byte total limit`);
    }
    return { ...journal, acquisition: "reused", destination, journalPath };
  }

  let lastError;
  for (let attempt = 0; attempt <= settings.retries; attempt += 1) {
    try {
      const result = await downloadAtomic({
        url: artifact.url,
        destination,
        root: settings.root,
        maxBytes: Math.min(settings.maxFileBytes, settings.maxTotalBytes),
        expectedBytes: artifact.advertisedBytes,
        timeoutMs: settings.timeoutMs,
        deadlineMs: settings.deadlineMs,
        networkPolicy: settings.networkPolicy,
      });
      settings.budget.used += result.bytes;
      if (!Number.isSafeInteger(settings.budget.used) || settings.budget.used > settings.maxTotalBytes) {
        await rm(destination, { force: true });
        fail(`bootstrap corpus exceeds the ${settings.maxTotalBytes}-byte total limit`);
      }
      const journal = {
        bytes: result.bytes,
        advertisedBytes: artifact.advertisedBytes,
        inventorySha256: settings.selectionBinding.source.inventorySha256,
        kind: artifact.kind,
        path: artifact.path,
        sourcePath: artifact.sourcePath,
        resolvedUrl: result.resolvedUrl,
        schemaVersion: 1,
        selectionSha256: settings.selectionBinding.selectionSha256,
        sha256: result.sha256,
        url: new URL(artifact.url).toString(),
      };
      await writeTextAtomic(journalPath, stableJson(journal));
      return { ...journal, acquisition: "downloaded", destination, journalPath };
    } catch (error) {
      lastError = error;
      if (attempt < settings.retries) {
        process.stderr.write(`retry ${artifact.path}: ${error instanceof Error ? error.message : String(error)}\n`);
      }
    }
  }
  throw lastError;
}

export function inspectEntries(command, artifact, timeoutMs, redaction = {}) {
  const result = spawnSync(
    command[0],
    [...command.slice(1), artifact.kind, artifact.destination, "--count-only"],
    { encoding: "utf8", maxBuffer: 64 * 1024, timeout: timeoutMs },
  );
  if (result.error) {
    const reason = result.error.code === "ETIMEDOUT"
      ? `metadata-only observation exceeded ${timeoutMs} ms`
      : `failed to run metadata-only observer: ${result.error.message}`;
    return { entries: null, error: errorMessage(reason, redaction) };
  }
  if (result.status !== 0) {
    return {
      entries: null,
      error: errorMessage(
        `mdictlib metadata-only observer rejected the artifact: ${(result.stderr || result.stdout).trim() || `exit ${result.status}`}`,
        redaction,
      ),
    };
  }
  const match = result.stdout.match(/^entries=([0-9]+)$/m);
  if (!match) return { entries: null, error: "mdictlib metadata-only observer did not report an entries line" };
  const entries = Number(match[1]);
  if (!Number.isSafeInteger(entries)) {
    return { entries: null, error: "mdictlib inspect reported an entry count outside the safe integer range" };
  }
  return { entries, error: null };
}

async function requireObserverSnapshotIdentity(command, observer, stage) {
  const identity = await sha256File(command[0]);
  if (identity.bytes !== observer.binaryBytes || identity.sha256 !== observer.binarySha256) {
    fail(`metadata-only observer snapshot changed ${stage} an artifact observation`);
  }
}

export async function inspectVerifiedEntries(command, artifact, {
  root,
  timeoutMs,
  observer,
  redaction = {},
}) {
  await verifyArtifact(root, artifact);
  await requireObserverSnapshotIdentity(command, observer, "before");
  const inspection = inspectEntries(command, artifact, timeoutMs, redaction);
  await requireObserverSnapshotIdentity(command, observer, "during");
  await verifyArtifact(root, artifact);
  return inspection;
}

function errorMessage(error, redaction = {}) {
  return sanitizeDiagnostic(error instanceof Error ? error.message : String(error), redaction);
}

async function readJsonPackageVersion(cargoTomlPath) {
  const text = await readFile(cargoTomlPath, "utf8");
  const packageHeader = /^\[package\]\s*$/m.exec(text);
  if (!packageHeader) fail(`failed to find [package] in ${cargoTomlPath}`);
  const afterHeader = text.slice(packageHeader.index + packageHeader[0].length);
  const nextSection = afterHeader.search(/^\[/m);
  const packageSection = nextSection === -1 ? afterHeader : afterHeader.slice(0, nextSection);
  const version = packageSection.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];
  if (!version) fail(`failed to read package version from ${cargoTomlPath}`);
  return version;
}

export async function main(argv = process.argv.slice(2), internal = {}) {
  const options = parseOptions(argv, {
    "--selection": "string",
    "--inventory": "string",
    "--output": "string",
    "--root": "string",
    "--concurrency": "string",
    "--retries": "string",
    "--timeout-ms": "string",
    "--deadline-ms": "string",
    "--observe-timeout-ms": "string",
    "--max-file-bytes": "string",
    "--max-total-bytes": "string",
    "--cargo": "string",
    "--skip-observe": "boolean",
  });
  if (!options.selection || !options.inventory || !options.output) {
    fail("usage: lock-corpus.mjs --selection <reviewed-selection.json> --inventory <source.inventory.json> --output <catalog.lock.draft.json> [options]");
  }
  const networkPolicy = internal.networkPolicy ?? {};
  const spawnBuildSync = internal.spawnSync ?? spawnSync;
  await assertDistinctPaths({ selection: options.selection, inventory: options.inventory, output: options.output });
  const [selectionIdentityBefore, inventoryIdentityBefore, selection, inventory] = await Promise.all([
    sha256File(options.selection),
    sha256File(options.inventory),
    readJson(options.selection),
    readJson(options.inventory),
  ]);
  const [selectionIdentityAfter, inventoryIdentityAfter] = await Promise.all([
    sha256File(options.selection),
    sha256File(options.inventory),
  ]);
  if (
    selectionIdentityBefore.sha256 !== selectionIdentityAfter.sha256 ||
    selectionIdentityBefore.bytes !== selectionIdentityAfter.bytes
  ) {
    fail("selection changed while it was being read");
  }
  if (
    inventoryIdentityBefore.sha256 !== inventoryIdentityAfter.sha256 ||
    inventoryIdentityBefore.bytes !== inventoryIdentityAfter.bytes
  ) {
    fail("inventory changed while it was being read");
  }
  validateSelectionAgainstInventory(
    selection,
    inventory,
    inventoryIdentityBefore.sha256,
    networkPolicy,
  );
  validateSelection(selection, networkPolicy);
  const canonicalSelection = stableJson(selection);
  if (
    selectionIdentityBefore.sha256 !== sha256Text(canonicalSelection) ||
    selectionIdentityBefore.bytes !== Buffer.byteLength(canonicalSelection)
  ) {
    fail("selection must use the canonical stable JSON serialization emitted by select-inventory.mjs");
  }
  const artifacts = selection.entries.flatMap((entry) =>
    entry.artifacts.map((artifact) => ({ entry, artifact })),
  );
  if (artifacts.length === 0) fail("selection has no artifacts");
  const maxFileBytes = positiveOption(options["max-file-bytes"], DEFAULT_MAX_FILE_BYTES, "--max-file-bytes");
  const maxTotalBytes = positiveOption(options["max-total-bytes"], DEFAULT_MAX_TOTAL_BYTES, "--max-total-bytes");
  const advertisedTotal = artifacts.reduce((sum, { artifact }) => {
    const next = sum + artifact.advertisedBytes;
    if (!Number.isSafeInteger(next)) fail("selection advertised byte total exceeds the safe integer range");
    return next;
  }, 0);
  const selectionBinding = {
    advertisedBytes: advertisedTotal,
    artifactCount: artifacts.length,
    artifactSetSha256: selectionArtifactSetSha256(artifacts.map(({ artifact }) => artifact)),
    entryCount: selection.entries.length,
    selectionSha256: selectionIdentityBefore.sha256,
    source: selection.source,
  };
  if (advertisedTotal > maxTotalBytes) {
    fail(`selection advertises ${advertisedTotal} bytes, exceeding the ${maxTotalBytes}-byte total limit`);
  }
  for (const { artifact } of artifacts) {
    if (artifact.advertisedBytes > maxFileBytes) {
      fail(`${artifact.path} advertises ${artifact.advertisedBytes} bytes, exceeding the ${maxFileBytes}-byte file limit`);
    }
  }
  const root = options.root ?? ".corpus";
  const repositoryRoot = fileURLToPath(new URL("../..", import.meta.url));
  await assertDistinctPaths({
    selection: options.selection,
    inventory: options.inventory,
    output: options.output,
    ...Object.fromEntries(
      artifacts.flatMap(({ artifact }, index) => {
        const destination = resolveCorpusPath(root, artifact.path);
        const state = downloadStatePaths(destination);
        return [
          [`artifact-${index}`, destination],
          [`journal-${index}`, `${destination}.mdictlib-lock.json`],
          [`partial-${index}`, state.partial],
          [`partial-metadata-${index}`, state.partialMetadata],
          [`partial-owner-${index}`, state.partialOwnership],
        ];
      }),
    ),
  });
  const concurrency = positiveOption(options.concurrency, 2, "--concurrency");
  const retries = positiveOption(options.retries, 2, "--retries", { allowZero: true });
  const timeoutMs = positiveOption(options["timeout-ms"], 120_000, "--timeout-ms");
  const deadlineMs = positiveOption(options["deadline-ms"], 6 * 60 * 60 * 1000, "--deadline-ms");
  const observeTimeoutMs = positiveOption(
    options["observe-timeout-ms"],
    5 * 60 * 1000,
    "--observe-timeout-ms",
  );
  await rm(options.output, { force: true });
  const budget = { used: 0 };
  let completed = 0;
  let completedBytes = 0;
  const acquisitionResults = await mapBounded(artifacts, concurrency, async ({ entry, artifact }) => {
    let result;
    let error = null;
    try {
      result = await acquire(artifact, {
        root,
        retries,
        timeoutMs,
        deadlineMs,
        maxFileBytes,
        maxTotalBytes,
        budget,
        networkPolicy,
        selectionBinding,
      });
      completedBytes += result.bytes;
    } catch (caught) {
      error = errorMessage(caught, {
        workspaceRoot: repositoryRoot,
        corpusRoot: root,
        target: resolveCorpusPath(root, artifact.path),
      });
    }
    completed += 1;
    process.stdout.write(
      `[acquire ${completed}/${artifacts.length}] ${result?.acquisition ?? "error"} ` +
        `${completedBytes}/${advertisedTotal} bytes ${artifact.path}\n`,
    );
    return { artifact, entry, error, result };
  });
  const acquired = acquisitionResults.filter(({ result }) => result !== undefined);
  const totalBytes = acquired.reduce((sum, { result }) => sum + result.bytes, 0);
  if (!Number.isSafeInteger(totalBytes) || totalBytes > maxTotalBytes) {
    fail(`acquired corpus is ${totalBytes} bytes, exceeding the ${maxTotalBytes}-byte total limit`);
  }

  let inspectCommand = null;
  let inspectBuildError = null;
  let inspectSnapshotDirectory = null;
  let observer = null;
  if (acquired.length > 0 && !options["skip-observe"]) {
    const cargo = options.cargo ?? "cargo";
    const build = spawnBuildSync(cargo, ["build", "--locked", "--release", "--all-features", "--example", "inspect"], {
      cwd: repositoryRoot,
      stdio: "inherit",
      timeout: Math.max(observeTimeoutMs, 10 * 60 * 1000),
    });
    if (build.error) {
      inspectBuildError = errorMessage(`failed to run ${cargo}: ${build.error.message}`, {
        workspaceRoot: repositoryRoot,
        corpusRoot: root,
      });
    } else if (build.status !== 0) {
      inspectBuildError = `cargo build for inspect exited with status ${build.status}`;
    } else {
      const configuredTarget = process.env.CARGO_TARGET_DIR;
      const targetRoot = configuredTarget
        ? path.resolve(repositoryRoot, configuredTarget)
        : path.join(repositoryRoot, "target");
      const builtInspect = path.join(
        targetRoot,
        "release",
        "examples",
        process.platform === "win32" ? "inspect.exe" : "inspect",
      );
      const builtIdentityBefore = await sha256File(builtInspect);
      inspectSnapshotDirectory = await mkdtemp(path.join(os.tmpdir(), "mdictlib-inspect-observer-"));
      const snapshottedInspect = path.join(
        inspectSnapshotDirectory,
        process.platform === "win32" ? "inspect.exe" : "inspect",
      );
      await copyFile(builtInspect, snapshottedInspect);
      if (process.platform !== "win32") await chmod(snapshottedInspect, 0o500);
      const [builtIdentityAfter, observerIdentity] = await Promise.all([
        sha256File(builtInspect),
        sha256File(snapshottedInspect),
      ]);
      if (
        builtIdentityBefore.bytes !== builtIdentityAfter.bytes ||
        builtIdentityBefore.sha256 !== builtIdentityAfter.sha256 ||
        observerIdentity.bytes !== builtIdentityBefore.bytes ||
        observerIdentity.sha256 !== builtIdentityBefore.sha256
      ) {
        fail("metadata-only observer binary changed while it was being snapshotted");
      }
      inspectCommand = [snapshottedInspect];
      const cargoToml = await readJsonPackageVersion(path.join(repositoryRoot, "Cargo.toml"));
      observer = {
        binaryBytes: observerIdentity.bytes,
        binarySha256: observerIdentity.sha256,
        mode: "metadata-open-and-count",
        timeoutMs: observeTimeoutMs,
        tool: "mdictlib/examples/inspect --count-only",
        version: cargoToml,
      };
    }
  }

  let inspected = 0;
  const lockedByPath = new Map();
  for (const { artifact, result: locked } of acquired) {
    let inspection;
    if (inspectCommand) {
      inspection = await inspectVerifiedEntries(inspectCommand, locked, {
        root,
        timeoutMs: observeTimeoutMs,
        observer,
        redaction: {
          workspaceRoot: repositoryRoot,
          corpusRoot: root,
          target: locked.destination,
          observerPath: inspectCommand[0],
        },
      });
    } else {
      inspection = {
        entries: null,
        error: options["skip-observe"] ? "observation skipped by explicit option" : inspectBuildError,
      };
    }
    inspected += 1;
    process.stdout.write(
      `[inspect ${inspected}/${acquired.length}] ${inspection.error === null ? "observed" : "error"} ${artifact.path}\n`,
    );
    lockedByPath.set(artifact.path, {
      bytes: locked.bytes,
      entryCountBasis: null,
      expectedEntries: null,
      keySha256: null,
      logicalDigestBasis: null,
      logicalObservation: null,
      kind: artifact.kind,
      observation: inspectCommand
        ? "mdictlib metadata-only open and entry count (self-derived; not independent verification)"
        : null,
      observer,
      observedEntries: inspection.entries,
      observationError: inspection.error,
      path: artifact.path,
      sourcePath: artifact.sourcePath,
      payloadSha256: null,
      resolvedUrl: locked.resolvedUrl,
      sha256: locked.sha256,
      url: locked.url,
    });
  }
  const draftEntries = selection.entries
    .map((entry) => ({
      ...entry,
      artifacts: entry.artifacts
        .map((artifact) => lockedByPath.get(artifact.path))
        .filter((artifact) => artifact !== undefined),
    }))
    .filter((entry) => entry.artifacts.length > 0);
  const draft = {
    acquisitionOutcomes: acquisitionResults.map(({ artifact, entry, error, result }) => ({
      acquisition: result?.acquisition ?? null,
      advertisedBytes: artifact.advertisedBytes,
      bytes: result?.bytes ?? null,
      entryId: entry.id,
      error,
      infoUrl: entry.infoUrl,
      kind: artifact.kind,
      path: artifact.path,
      sourcePath: artifact.sourcePath,
      resolvedUrl: result?.resolvedUrl ?? null,
      review: entry.review,
      sha256: result?.sha256 ?? null,
      sourceTitle: entry.title,
      status: result ? "acquired" : "acquisition-error",
      url: new URL(artifact.url).toString(),
    })),
    catalog: selection.catalog,
    entries: draftEntries,
    schemaVersion: 1,
    selectionBinding,
  };
  await writeTextAtomic(options.output, stableJson(draft));
  if (inspectSnapshotDirectory !== null) {
    await rm(inspectSnapshotDirectory, { recursive: true, force: true });
  }
  const acquisitionErrors = acquisitionResults.length - acquired.length;
  process.stdout.write(
    `Wrote bootstrap draft for ${acquired.length}/${artifacts.length} acquired artifacts ` +
      `(${totalBytes} bytes, ${acquisitionErrors} acquisition errors) to ${options.output}.\n` +
      "This draft is intentionally not a valid catalog lock: promote successful observations explicitly and retain the local journals for resumable re-observation.\n",
  );
  return { acquired: acquired.length, acquisitionErrors, draft, totalBytes };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`lock-corpus: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
