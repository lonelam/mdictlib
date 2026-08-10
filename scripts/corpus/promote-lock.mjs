#!/usr/bin/env node

import process from "node:process";
import { fileURLToPath, pathToFileURL } from "node:url";
import {
  assertDistinctPaths,
  assertExactKeys,
  fail,
  parseOptions,
  requireAcquisitionUrl,
  requireHttpUrl,
  requireRelativePath,
  requireSafeInteger,
  requireString,
  readJson,
  sanitizeDiagnostic,
  selectionArtifactSetSha256,
  sha256File,
  sha256Text,
  stableJson,
  validateLock,
  validateObserver,
  validateReview,
  validateSelectionBinding,
  writeTextAtomic,
} from "./lib.mjs";

const SHA256 = /^[0-9a-f]{64}$/;
const ENTRY_ID = /^[a-z0-9](?:[a-z0-9._-]{0,126}[a-z0-9])?$/;
const REPOSITORY_ROOT = fileURLToPath(new URL("../..", import.meta.url));

function requireSha256(value, where) {
  if (typeof value !== "string" || !SHA256.test(value)) {
    fail(`${where} must be 64 lowercase hexadecimal digits`);
  }
}

function validateDraftArtifact(artifact, where, networkPolicy) {
  const fields = [
    "bytes",
    "entryCountBasis",
    "expectedEntries",
    "keySha256",
    "kind",
    "logicalDigestBasis",
    "logicalObservation",
    "observation",
    "observationError",
    "observedEntries",
    "observer",
    "path",
    "payloadSha256",
    "resolvedUrl",
    "sha256",
    "sourcePath",
    "url",
  ];
  assertExactKeys(artifact, fields, fields, where);
  if (!["mdx", "mdd"].includes(artifact.kind)) fail(`${where}.kind must be mdx or mdd`);
  requireString(artifact.sourcePath, `${where}.sourcePath`);
  if (/[\t\r\n\0]/.test(artifact.sourcePath)) fail(`${where}.sourcePath contains a forbidden control character`);
  requireRelativePath(artifact.path, artifact.kind, `${where}.path`);
  requireAcquisitionUrl(artifact.url, `${where}.url`, networkPolicy);
  requireAcquisitionUrl(artifact.resolvedUrl, `${where}.resolvedUrl`, networkPolicy);
  if (new URL(artifact.url).origin !== new URL(artifact.resolvedUrl).origin) {
    fail(`${where}.resolvedUrl must remain on the reviewed URL origin`);
  }
  requireSafeInteger(artifact.bytes, `${where}.bytes`, 1);
  requireSha256(artifact.sha256, `${where}.sha256`);
  if (artifact.expectedEntries !== null || artifact.entryCountBasis !== null) {
    fail(`${where} is already promoted; expectedEntries and entryCountBasis must be null`);
  }
  for (const field of ["keySha256", "payloadSha256", "logicalDigestBasis", "logicalObservation"]) {
    if (artifact[field] !== null) fail(`${where}.${field} must be null in a bootstrap draft`);
  }
  if (artifact.observedEntries !== null) {
    requireSafeInteger(artifact.observedEntries, `${where}.observedEntries`);
  }
  if (artifact.observation !== null) requireString(artifact.observation, `${where}.observation`);
  if (artifact.observationError !== null) requireString(artifact.observationError, `${where}.observationError`);
  validateObserver(artifact.observer, `${where}.observer`);
  if (artifact.observedEntries !== null) {
    if (artifact.observation === null || artifact.observationError !== null || artifact.observer === null) {
      fail(`${where}.observedEntries requires successful observation provenance`);
    }
  }
  return artifact;
}

function validateBootstrapDraft(draft, networkPolicy) {
  const topFields = ["schemaVersion", "selectionBinding", "catalog", "entries", "acquisitionOutcomes"];
  assertExactKeys(draft, topFields, topFields, "draft");
  if (draft.schemaVersion !== 1 || !Array.isArray(draft.entries) || !Array.isArray(draft.acquisitionOutcomes)) {
    fail("draft must have schemaVersion 1 plus entries and acquisitionOutcomes arrays");
  }
  validateSelectionBinding(draft.selectionBinding, "draft.selectionBinding", networkPolicy);

  const draftArtifacts = new Map();
  const entryIds = new Set();
  for (const [entryIndex, entry] of draft.entries.entries()) {
    const where = `draft.entries[${entryIndex}]`;
    const entryFields = ["id", "title", "infoUrl", "review", "artifacts"];
    assertExactKeys(entry, entryFields, entryFields, where);
    requireString(entry.id, `${where}.id`);
    if (!ENTRY_ID.test(entry.id)) fail(`${where}.id is not a stable lowercase identifier`);
    if (!entryIds.add(entry.id)) fail(`${where}.id duplicates ${entry.id}`);
    requireString(entry.title, `${where}.title`);
    requireHttpUrl(entry.infoUrl, `${where}.infoUrl`);
    validateReview(entry.review, `${where}.review`);
    if (!Array.isArray(entry.artifacts) || entry.artifacts.length !== 1) {
      fail(`${where}.artifacts must contain exactly one acquired source artifact`);
    }
    const artifact = validateDraftArtifact(entry.artifacts[0], `${where}.artifacts[0]`, networkPolicy);
    if (draftArtifacts.has(artifact.path)) fail(`${where}.artifacts[0].path duplicates ${artifact.path}`);
    draftArtifacts.set(artifact.path, { artifact, entry });
  }

  const outcomePaths = new Set();
  const outcomeSourcePaths = new Set();
  const outcomeEntryIds = new Set();
  const denominatorRows = [];
  let advertisedBytes = 0;
  for (const [index, outcome] of draft.acquisitionOutcomes.entries()) {
    const where = `draft.acquisitionOutcomes[${index}]`;
    const fields = [
      "acquisition",
      "advertisedBytes",
      "bytes",
      "entryId",
      "error",
      "infoUrl",
      "kind",
      "path",
      "resolvedUrl",
      "review",
      "sha256",
      "sourcePath",
      "sourceTitle",
      "status",
      "url",
    ];
    assertExactKeys(outcome, fields, fields, where);
    requireString(outcome.entryId, `${where}.entryId`);
    if (!ENTRY_ID.test(outcome.entryId)) fail(`${where}.entryId is not a stable lowercase identifier`);
    if (!outcomeEntryIds.add(outcome.entryId)) fail(`${where}.entryId duplicates ${outcome.entryId}`);
    requireString(outcome.sourceTitle, `${where}.sourceTitle`);
    requireHttpUrl(outcome.infoUrl, `${where}.infoUrl`);
    validateReview(outcome.review, `${where}.review`);
    if (!["mdx", "mdd"].includes(outcome.kind)) fail(`${where}.kind must be mdx or mdd`);
    if (outcome.kind !== draft.selectionBinding.source.selectedType) {
      fail(`${where}.kind differs from the bound source type`);
    }
    requireString(outcome.sourcePath, `${where}.sourcePath`);
    if (/[\t\r\n\0]/.test(outcome.sourcePath)) fail(`${where}.sourcePath contains a forbidden control character`);
    requireRelativePath(outcome.path, outcome.kind, `${where}.path`);
    requireAcquisitionUrl(outcome.url, `${where}.url`, networkPolicy);
    requireSafeInteger(outcome.advertisedBytes, `${where}.advertisedBytes`, 1);
    if (!outcomePaths.add(outcome.path)) fail(`${where}.path duplicates ${outcome.path}`);
    if (!outcomeSourcePaths.add(outcome.sourcePath)) fail(`${where}.sourcePath duplicates ${outcome.sourcePath}`);
    advertisedBytes += outcome.advertisedBytes;
    if (!Number.isSafeInteger(advertisedBytes)) fail("draft outcome advertised byte total exceeds the safe integer range");
    denominatorRows.push(outcome);

    const draftRow = draftArtifacts.get(outcome.path);
    if (outcome.status === "acquired") {
      if (!["downloaded", "reused"].includes(outcome.acquisition)) {
        fail(`${where}.acquisition must be downloaded or reused`);
      }
      requireSafeInteger(outcome.bytes, `${where}.bytes`, 1);
      if (outcome.bytes !== outcome.advertisedBytes) fail(`${where}.bytes differs from advertisedBytes`);
      requireAcquisitionUrl(outcome.resolvedUrl, `${where}.resolvedUrl`, networkPolicy);
      if (new URL(outcome.url).origin !== new URL(outcome.resolvedUrl).origin) {
        fail(`${where}.resolvedUrl must remain on the reviewed URL origin`);
      }
      requireSha256(outcome.sha256, `${where}.sha256`);
      if (outcome.error !== null) fail(`${where}.error must be null when acquired`);
      if (!draftRow) fail(`${where} acquired artifact is missing from draft.entries`);
      if (
        draftRow.entry.id !== outcome.entryId ||
        draftRow.entry.title !== outcome.sourceTitle ||
        draftRow.entry.infoUrl !== outcome.infoUrl ||
        stableJson(draftRow.entry.review) !== stableJson(outcome.review) ||
        draftRow.artifact.kind !== outcome.kind ||
        draftRow.artifact.sourcePath !== outcome.sourcePath ||
        draftRow.artifact.url !== outcome.url ||
        draftRow.artifact.resolvedUrl !== outcome.resolvedUrl ||
        draftRow.artifact.bytes !== outcome.bytes ||
        draftRow.artifact.sha256 !== outcome.sha256
      ) {
        fail(`${where} does not match its acquired draft artifact`);
      }
    } else if (outcome.status === "acquisition-error") {
      if (
        outcome.acquisition !== null ||
        outcome.bytes !== null ||
        outcome.resolvedUrl !== null ||
        outcome.sha256 !== null
      ) {
        fail(`${where} acquisition-error facts must be null`);
      }
      requireString(outcome.error, `${where}.error`);
      if (draftRow) fail(`${where} acquisition-error artifact must not appear in draft.entries`);
    } else {
      fail(`${where}.status must be acquired or acquisition-error`);
    }
  }

  const binding = draft.selectionBinding;
  if (draft.acquisitionOutcomes.length !== binding.artifactCount) {
    fail(`draft has ${draft.acquisitionOutcomes.length} outcomes; bound selection requires ${binding.artifactCount}`);
  }
  if (outcomeEntryIds.size !== binding.entryCount) {
    fail(`draft has ${outcomeEntryIds.size} outcome entry IDs; bound selection requires ${binding.entryCount}`);
  }
  if (advertisedBytes !== binding.advertisedBytes) {
    fail(`draft outcomes advertise ${advertisedBytes} bytes; bound selection requires ${binding.advertisedBytes}`);
  }
  if (selectionArtifactSetSha256(denominatorRows) !== binding.artifactSetSha256) {
    fail("draft outcome artifact set does not match the bound reviewed selection");
  }
  const reconstructedSelection = {
    catalog: draft.catalog,
    entries: draft.acquisitionOutcomes.map((outcome) => ({
      artifacts: [{
        advertisedBytes: outcome.advertisedBytes,
        kind: outcome.kind,
        path: outcome.path,
        sourcePath: outcome.sourcePath,
        url: outcome.url,
      }],
      id: outcome.entryId,
      infoUrl: outcome.infoUrl,
      review: outcome.review,
      title: outcome.sourceTitle,
    })),
    schemaVersion: 1,
    source: binding.source,
  };
  if (sha256Text(stableJson(reconstructedSelection)) !== binding.selectionSha256) {
    fail("draft selection facts do not match the exact bound canonical selection bytes");
  }
  for (const artifactPath of draftArtifacts.keys()) {
    if (!outcomePaths.has(artifactPath)) fail(`draft artifact ${artifactPath} has no acquisition outcome`);
  }
  return draft;
}

export function promoteDraft(rawDraft, networkPolicy = {}) {
  const draft = validateBootstrapDraft(rawDraft, networkPolicy);
  const draftByPath = new Map();
  const entries = [];
  for (const entry of draft.entries) {
    const promotedArtifacts = [];
    for (const artifact of entry.artifacts) {
      const successful = artifact.observedEntries !== null && artifact.observationError === null;
      draftByPath.set(artifact.path, { artifact, successful });
      if (successful) {
        promotedArtifacts.push({
          ...artifact,
          entryCountBasis: "mdictlib-self-observed",
          expectedEntries: artifact.observedEntries,
        });
      }
    }
    if (promotedArtifacts.length > 0) entries.push({ ...entry, artifacts: promotedArtifacts });
  }
  const results = draft.acquisitionOutcomes.map((outcome) => {
    const sanitizedError = outcome.error === null
      ? null
      : sanitizeDiagnostic(outcome.error, { workspaceRoot: REPOSITORY_ROOT });
    if (outcome.status === "acquisition-error") {
      return {
        ...outcome,
        error: sanitizedError,
        entryCountBasis: null,
        observation: null,
        observationError: null,
        observedEntries: null,
        observer: null,
      };
    }
    const row = draftByPath.get(outcome.path);
    if (!row) fail(`acquired outcome ${outcome.path} has no draft observation row`);
    const { artifact, successful } = row;
    return {
      ...outcome,
      error: sanitizedError,
      entryCountBasis: successful ? "mdictlib-self-observed" : null,
      observation: artifact.observation,
      observationError: artifact.observationError === null
        ? null
        : sanitizeDiagnostic(artifact.observationError, { workspaceRoot: REPOSITORY_ROOT }),
      observedEntries: artifact.observedEntries,
      observer: artifact.observer,
      status: successful ? "promoted" : "excluded",
    };
  });
  const lock = validateLock({ catalog: draft.catalog, entries, schemaVersion: 1 }, networkPolicy);
  const canonicalLock = stableJson(lock);
  return {
    lock,
    outcomes: {
      catalog: draft.catalog,
      promotedLock: {
        bytes: Buffer.byteLength(canonicalLock),
        sha256: sha256Text(canonicalLock),
      },
      results,
      schemaVersion: 1,
      selectionBinding: draft.selectionBinding,
      scope:
        "Complete bootstrap acquisition and parser outcomes for the exact bound reviewed selection. Promoted rows are self-recorded regression baselines, not independent correctness evidence; acquisition and inspection failures remain explicit.",
    },
  };
}

export function validatePromotionPair(
  rawLock,
  rawOutcomes,
  networkPolicy = {},
  { requireSourceDraft = false } = {},
) {
  const lock = validateLock(rawLock, networkPolicy);
  const outcomeFields = [
    "catalog",
    "promotedLock",
    "results",
    "schemaVersion",
    "selectionBinding",
    "scope",
    "sourceDraftBytes",
    "sourceDraftSha256",
  ];
  assertExactKeys(
    rawOutcomes,
    outcomeFields,
    requireSourceDraft ? outcomeFields : outcomeFields.filter((field) => !field.startsWith("sourceDraft")),
    "promotion outcomes",
  );
  if (rawOutcomes.schemaVersion !== 1 || !Array.isArray(rawOutcomes.results)) {
    fail("promotion outcomes must have schemaVersion 1 and a results array");
  }
  requireString(rawOutcomes.scope, "promotion outcomes.scope");
  validateSelectionBinding(rawOutcomes.selectionBinding, "promotion outcomes.selectionBinding", networkPolicy);
  if (stableJson(rawOutcomes.catalog) !== stableJson(lock.catalog)) {
    fail("promoted lock catalog differs from the complete outcomes catalog");
  }
  assertExactKeys(
    rawOutcomes.promotedLock,
    ["bytes", "sha256"],
    ["bytes", "sha256"],
    "promotion outcomes.promotedLock",
  );
  requireSafeInteger(rawOutcomes.promotedLock.bytes, "promotion outcomes.promotedLock.bytes", 1);
  requireSha256(rawOutcomes.promotedLock.sha256, "promotion outcomes.promotedLock.sha256");
  const canonicalLock = stableJson(lock);
  if (
    rawOutcomes.promotedLock.bytes !== Buffer.byteLength(canonicalLock) ||
    rawOutcomes.promotedLock.sha256 !== sha256Text(canonicalLock)
  ) {
    fail("promoted lock bytes do not match the lock identity recorded by the complete outcomes");
  }
  if (requireSourceDraft) {
    requireSafeInteger(rawOutcomes.sourceDraftBytes, "promotion outcomes.sourceDraftBytes", 1);
    requireSha256(rawOutcomes.sourceDraftSha256, "promotion outcomes.sourceDraftSha256");
  }

  const binding = rawOutcomes.selectionBinding;
  if (rawOutcomes.results.length !== binding.artifactCount) {
    fail(`promotion outcomes have ${rawOutcomes.results.length} rows; bound selection requires ${binding.artifactCount}`);
  }
  const resultPaths = new Set();
  const resultEntryIds = new Set();
  let advertisedBytes = 0;
  for (const [index, result] of rawOutcomes.results.entries()) {
    const where = `promotion outcomes.results[${index}]`;
    const resultFields = [
      "acquisition",
      "advertisedBytes",
      "bytes",
      "entryCountBasis",
      "entryId",
      "error",
      "infoUrl",
      "kind",
      "observation",
      "observationError",
      "observedEntries",
      "observer",
      "path",
      "resolvedUrl",
      "review",
      "sha256",
      "sourcePath",
      "sourceTitle",
      "status",
      "url",
    ];
    assertExactKeys(result, resultFields, resultFields, where);
    requireString(result.entryId, `${where}.entryId`);
    if (!ENTRY_ID.test(result.entryId) || !resultEntryIds.add(result.entryId)) {
      fail(`${where}.entryId must be a unique stable lowercase identifier`);
    }
    requireString(result.sourceTitle, `${where}.sourceTitle`);
    requireHttpUrl(result.infoUrl, `${where}.infoUrl`);
    validateReview(result.review, `${where}.review`);
    if (!["mdx", "mdd"].includes(result.kind)) fail(`${where}.kind must be mdx or mdd`);
    requireRelativePath(result.path, result.kind, `${where}.path`);
    if (!resultPaths.add(result.path)) fail(`${where}.path duplicates ${result.path}`);
    requireString(result.sourcePath, `${where}.sourcePath`);
    requireAcquisitionUrl(result.url, `${where}.url`, networkPolicy);
    requireSafeInteger(result.advertisedBytes, `${where}.advertisedBytes`, 1);
    advertisedBytes += result.advertisedBytes;
    if (!Number.isSafeInteger(advertisedBytes)) fail("promotion outcome byte total exceeds the safe integer range");
    if (!["promoted", "excluded", "acquisition-error"].includes(result.status)) {
      fail(`${where}.status must be promoted, excluded, or acquisition-error`);
    }
    if (result.status === "acquisition-error") {
      if (
        result.acquisition !== null ||
        result.bytes !== null ||
        result.resolvedUrl !== null ||
        result.sha256 !== null ||
        result.entryCountBasis !== null ||
        result.observation !== null ||
        result.observationError !== null ||
        result.observedEntries !== null ||
        result.observer !== null
      ) {
        fail(`${where} acquisition-error provenance fields must be null`);
      }
      requireString(result.error, `${where}.error`);
      continue;
    }
    if (!["downloaded", "reused"].includes(result.acquisition)) {
      fail(`${where}.acquisition must be downloaded or reused`);
    }
    requireSafeInteger(result.bytes, `${where}.bytes`, 1);
    if (result.bytes !== result.advertisedBytes) {
      fail(`${where}.bytes differs from advertisedBytes`);
    }
    requireAcquisitionUrl(result.resolvedUrl, `${where}.resolvedUrl`, networkPolicy);
    if (new URL(result.url).origin !== new URL(result.resolvedUrl).origin) {
      fail(`${where}.resolvedUrl must remain on the reviewed URL origin`);
    }
    requireSha256(result.sha256, `${where}.sha256`);
    if (result.error !== null) fail(`${where}.error must be null after successful acquisition`);
    if (result.status === "promoted") {
      if (result.entryCountBasis !== "mdictlib-self-observed") {
        fail(`${where}.entryCountBasis must be mdictlib-self-observed`);
      }
      requireSafeInteger(result.observedEntries, `${where}.observedEntries`);
      requireString(result.observation, `${where}.observation`);
      if (result.observationError !== null) fail(`${where}.observationError must be null`);
      if (result.observer === null) fail(`${where}.observer is required for a promoted observation`);
      validateObserver(result.observer, `${where}.observer`);
    } else {
      if (result.entryCountBasis !== null || result.observedEntries !== null) {
        fail(`${where} excluded entry-count fields must be null`);
      }
      if (result.observation !== null) requireString(result.observation, `${where}.observation`);
      requireString(result.observationError, `${where}.observationError`);
      validateObserver(result.observer, `${where}.observer`);
    }
  }
  if (resultEntryIds.size !== binding.entryCount || advertisedBytes !== binding.advertisedBytes) {
    fail("promotion outcomes do not match the bound selection denominator");
  }
  if (selectionArtifactSetSha256(rawOutcomes.results) !== binding.artifactSetSha256) {
    fail("promotion outcome artifact set differs from the bound reviewed selection");
  }
  const reconstructedSelection = {
    catalog: rawOutcomes.catalog,
    entries: rawOutcomes.results.map((result) => ({
      artifacts: [{
        advertisedBytes: result.advertisedBytes,
        kind: result.kind,
        path: result.path,
        sourcePath: result.sourcePath,
        url: result.url,
      }],
      id: result.entryId,
      infoUrl: result.infoUrl,
      review: result.review,
      title: result.sourceTitle,
    })),
    schemaVersion: 1,
    source: binding.source,
  };
  if (sha256Text(stableJson(reconstructedSelection)) !== binding.selectionSha256) {
    fail("promotion outcome review facts differ from the exact bound reviewed selection");
  }

  const lockByPath = new Map();
  for (const entry of lock.entries) {
    for (const artifact of entry.artifacts) {
      if (lockByPath.has(artifact.path)) fail(`promoted lock duplicates ${artifact.path}`);
      lockByPath.set(artifact.path, { artifact, entry });
    }
  }
  let promoted = 0;
  for (const result of rawOutcomes.results) {
    const locked = lockByPath.get(result.path);
    if (result.status !== "promoted") {
      if (locked) fail(`non-promoted outcome ${result.path} appears in the promoted lock`);
      continue;
    }
    promoted += 1;
    if (!locked) fail(`promoted outcome ${result.path} is absent from the promoted lock`);
    const { artifact, entry } = locked;
    if (
      artifact.keySha256 !== null ||
      artifact.payloadSha256 !== null ||
      artifact.logicalDigestBasis !== null ||
      artifact.logicalObservation !== null
    ) {
      fail(`promoted lock artifact ${result.path} contains logical baselines absent from bootstrap outcomes`);
    }
    if (
      entry.id !== result.entryId ||
      entry.title !== result.sourceTitle ||
      entry.infoUrl !== result.infoUrl ||
      stableJson(entry.review) !== stableJson(result.review) ||
      artifact.kind !== result.kind ||
      artifact.path !== result.path ||
      artifact.sourcePath !== result.sourcePath ||
      artifact.url !== result.url ||
      artifact.resolvedUrl !== result.resolvedUrl ||
      artifact.bytes !== result.bytes ||
      artifact.sha256 !== result.sha256 ||
      artifact.expectedEntries !== result.observedEntries ||
      artifact.entryCountBasis !== result.entryCountBasis ||
      artifact.observation !== result.observation ||
      artifact.observationError !== result.observationError ||
      artifact.observedEntries !== result.observedEntries ||
      stableJson(artifact.observer) !== stableJson(result.observer)
    ) {
      fail(`promoted lock artifact ${result.path} differs from its complete outcome`);
    }
  }
  if (lockByPath.size !== promoted) fail("promoted lock contains artifacts absent from promoted outcomes");
  return { lock, outcomes: rawOutcomes };
}

export async function main(argv = process.argv.slice(2), internal = {}) {
  const options = parseOptions(argv, {
    "--input": "string",
    "--output": "string",
    "--outcomes": "string",
    "--accept-self-observed": "boolean",
    "--verify-pair": "boolean",
  });
  if (options["verify-pair"]) {
    if (!options.output || !options.outcomes || options.input || options["accept-self-observed"]) {
      fail("usage: promote-lock.mjs --verify-pair --output <catalog.lock.json> --outcomes <outcomes.json>");
    }
    await assertDistinctPaths({ output: options.output, outcomes: options.outcomes });
    const [lock, outcomes] = await Promise.all([readJson(options.output), readJson(options.outcomes)]);
    validatePromotionPair(lock, outcomes, internal.networkPolicy ?? {}, { requireSourceDraft: true });
    process.stdout.write(`Verified promoted lock/outcomes pair: ${options.output} + ${options.outcomes}\n`);
    return;
  }
  if (!options.input || !options.output || !options.outcomes) {
    fail("usage: promote-lock.mjs --input <catalog.lock.draft.json> --output <catalog.lock.json> --outcomes <outcomes.json> --accept-self-observed");
  }
  if (!options["accept-self-observed"]) {
    fail("promotion requires --accept-self-observed; parser-derived counts are never accepted implicitly");
  }
  await assertDistinctPaths({ input: options.input, output: options.output, outcomes: options.outcomes });
  const [draft, identityBefore] = await Promise.all([readJson(options.input), sha256File(options.input)]);
  const identityAfter = await sha256File(options.input);
  if (identityBefore.bytes !== identityAfter.bytes || identityBefore.sha256 !== identityAfter.sha256) {
    fail("bootstrap draft changed while it was being read");
  }
  const { lock, outcomes } = promoteDraft(draft, internal.networkPolicy ?? {});
  outcomes.sourceDraftBytes = identityBefore.bytes;
  outcomes.sourceDraftSha256 = identityBefore.sha256;
  await writeTextAtomic(options.output, stableJson(lock));
  await writeTextAtomic(options.outcomes, stableJson(outcomes));
  const [writtenLock, writtenOutcomes] = await Promise.all([
    readJson(options.output),
    readJson(options.outcomes),
  ]);
  validatePromotionPair(writtenLock, writtenOutcomes, internal.networkPolicy ?? {}, {
    requireSourceDraft: true,
  });
  const promoted = outcomes.results.filter(({ status }) => status === "promoted").length;
  const acquisitionErrors = outcomes.results.filter(({ status }) => status === "acquisition-error").length;
  const excluded = outcomes.results.length - promoted - acquisitionErrors;
  process.stdout.write(
    `Promoted ${promoted} self-observed artifacts; excluded ${excluded}; ` +
      `retained ${acquisitionErrors} acquisition errors.\n` +
      `Lock: ${options.output}\nOutcomes: ${options.outcomes}\n` +
      "These entry counts are mdictlib regression snapshots, not independent parser validation.\n",
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`promote-lock: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
