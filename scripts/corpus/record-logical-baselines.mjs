#!/usr/bin/env node

import { readFile } from "node:fs/promises";
import process from "node:process";
import { pathToFileURL } from "node:url";
import {
  AUDIT_PROTOCOL,
  CORPUS_AUDIT_TOOL,
  OUTCOMES_SCHEMA_VERSION,
  exhaustiveDenominator,
} from "./audit-corpus.mjs";
import {
  approvedArtifacts,
  assertExactKeys,
  assertDistinctPaths,
  fail,
  parseOptions,
  readJson,
  requireRelativePath,
  requireSafeInteger,
  requireString,
  sha256File,
  sha256Text,
  stableJson,
  validateLock,
  writeTextAtomic,
} from "./lib.mjs";

const HEADER = "path\tkind\tentries\tkey_sha256\tpayload_sha256";
const SHA256 = /^[0-9a-f]{64}$/;

function parseAudit(text) {
  const rows = new Map();
  let sawHeader = false;
  for (const [index, rawLine] of text.split(/\n/).entries()) {
    const line = rawLine.endsWith("\r") ? rawLine.slice(0, -1) : rawLine;
    if (line === "" || line.startsWith("#")) continue;
    if (!sawHeader) {
      if (line !== HEADER) fail(`audit line ${index + 1}: expected header ${JSON.stringify(HEADER)}`);
      sawHeader = true;
      continue;
    }
    const fields = line.split("\t");
    if (fields.length !== 5) fail(`audit line ${index + 1}: expected five tab-separated fields`);
    const [relativePath, kind, entriesText, keySha256, payloadSha256] = fields;
    if (!["mdx", "mdd"].includes(kind)) fail(`audit line ${index + 1}: kind must be mdx or mdd`);
    requireRelativePath(relativePath, kind, `audit line ${index + 1} path`);
    if (!/^[0-9]+$/.test(entriesText)) fail(`audit line ${index + 1}: entries must be an unsigned integer`);
    const entries = Number(entriesText);
    if (!Number.isSafeInteger(entries)) fail(`audit line ${index + 1}: entries exceeds the safe integer range`);
    if (!SHA256.test(keySha256) || !SHA256.test(payloadSha256)) {
      fail(`audit line ${index + 1}: logical digests must be 64 lowercase hexadecimal digits`);
    }
    if (rows.has(relativePath)) fail(`audit line ${index + 1}: duplicate path ${relativePath}`);
    rows.set(relativePath, { entries, keySha256, kind, payloadSha256 });
  }
  if (!sawHeader) fail("audit TSV has no header");
  return rows;
}

function validateIdentity(identity, where) {
  assertExactKeys(identity, ["bytes", "sha256"], ["bytes", "sha256"], where);
  requireSafeInteger(identity.bytes, `${where}.bytes`, 1);
  if (!SHA256.test(identity.sha256)) {
    fail(`${where}.sha256 must be 64 lowercase hexadecimal digits`);
  }
  return identity;
}

function canonicalAuditText(results) {
  return `${[
    HEADER,
    ...results.map((result) =>
      [
        result.path,
        result.kind,
        String(result.audit.entries),
        result.audit.keySha256,
        result.audit.payloadSha256,
      ].join("\t"),
    ),
  ].join("\n")}\n`;
}

export function validateExhaustiveEvidence({
  lock,
  auditText,
  outcomes,
  catalogIdentity,
  outcomesIdentity,
}) {
  validateIdentity(catalogIdentity, "catalog identity");
  validateIdentity(outcomesIdentity, "outcomes identity");
  assertExactKeys(
    outcomes,
    [
      "audit",
      "catalog",
      "completeSuccess",
      "denominator",
      "execution",
      "generatedAt",
      "protocol",
      "results",
      "runner",
      "schemaVersion",
      "summary",
    ],
    [
      "audit",
      "catalog",
      "completeSuccess",
      "denominator",
      "execution",
      "generatedAt",
      "protocol",
      "results",
      "runner",
      "schemaVersion",
      "summary",
    ],
    "exhaustive outcomes",
  );
  if (outcomes.schemaVersion !== OUTCOMES_SCHEMA_VERSION) {
    fail(`exhaustive outcomes.schemaVersion must be ${OUTCOMES_SCHEMA_VERSION}`);
  }
  if (outcomes.protocol !== AUDIT_PROTOCOL) {
    fail(`exhaustive outcomes.protocol must be ${AUDIT_PROTOCOL}`);
  }
  requireString(outcomes.generatedAt, "exhaustive outcomes.generatedAt");
  if (!Number.isFinite(Date.parse(outcomes.generatedAt))) {
    fail("exhaustive outcomes.generatedAt is not an ISO date-time");
  }
  if (outcomes.completeSuccess !== true) {
    fail("exhaustive outcomes must record completeSuccess=true before baseline promotion");
  }
  const canonicalOutcomes = stableJson(outcomes);
  if (
    outcomesIdentity.bytes !== Buffer.byteLength(canonicalOutcomes) ||
    outcomesIdentity.sha256 !== sha256Text(canonicalOutcomes)
  ) {
    fail("exhaustive outcomes identity does not match its canonical JSON bytes");
  }

  validateIdentity(outcomes.catalog, "exhaustive outcomes.catalog");
  if (
    outcomes.catalog.bytes !== catalogIdentity.bytes ||
    outcomes.catalog.sha256 !== catalogIdentity.sha256
  ) {
    fail("exhaustive outcomes catalog identity does not match the input catalog bytes");
  }
  assertExactKeys(
    outcomes.runner,
    ["binaryBytes", "binarySha256", "protocol", "tool", "version"],
    ["binaryBytes", "binarySha256", "protocol", "tool", "version"],
    "exhaustive outcomes.runner",
  );
  requireSafeInteger(outcomes.runner.binaryBytes, "exhaustive outcomes.runner.binaryBytes", 1);
  if (!SHA256.test(outcomes.runner.binarySha256)) {
    fail("exhaustive outcomes.runner.binarySha256 must be 64 lowercase hexadecimal digits");
  }
  if (outcomes.runner.protocol !== AUDIT_PROTOCOL) {
    fail(`exhaustive outcomes.runner.protocol must be ${AUDIT_PROTOCOL}`);
  }
  if (outcomes.runner.tool !== CORPUS_AUDIT_TOOL) {
    fail(`exhaustive outcomes.runner.tool must be ${CORPUS_AUDIT_TOOL}`);
  }
  requireString(outcomes.runner.version, "exhaustive outcomes.runner.version");

  const expectedDenominator = exhaustiveDenominator(lock);
  assertExactKeys(
    outcomes.denominator,
    ["artifactCount", "artifacts", "sha256"],
    ["artifactCount", "artifacts", "sha256"],
    "exhaustive outcomes.denominator",
  );
  requireSafeInteger(outcomes.denominator.artifactCount, "exhaustive outcomes.denominator.artifactCount", 1);
  if (outcomes.denominator.artifactCount !== expectedDenominator.length) {
    fail("exhaustive outcomes denominator count does not match the input catalog");
  }
  if (stableJson(outcomes.denominator.artifacts) !== stableJson(expectedDenominator)) {
    fail("exhaustive outcomes artifact denominator does not exactly match the input catalog");
  }
  const denominatorSha256 = sha256Text(stableJson(expectedDenominator));
  if (outcomes.denominator.sha256 !== denominatorSha256) {
    fail("exhaustive outcomes denominator SHA-256 is invalid");
  }

  assertExactKeys(
    outcomes.execution,
    ["artifactTimeoutMs", "concurrency"],
    ["artifactTimeoutMs", "concurrency"],
    "exhaustive outcomes.execution",
  );
  requireSafeInteger(outcomes.execution.artifactTimeoutMs, "exhaustive outcomes.execution.artifactTimeoutMs", 1);
  requireSafeInteger(outcomes.execution.concurrency, "exhaustive outcomes.execution.concurrency", 1);
  assertExactKeys(
    outcomes.summary,
    ["failed", "passed"],
    ["failed", "passed"],
    "exhaustive outcomes.summary",
  );
  if (outcomes.summary.failed !== 0 || outcomes.summary.passed !== expectedDenominator.length) {
    fail("exhaustive outcomes summary is not an exact complete-success denominator");
  }
  if (!Array.isArray(outcomes.results) || outcomes.results.length !== expectedDenominator.length) {
    fail("exhaustive outcomes results are not the exact catalog denominator");
  }
  for (const [index, result] of outcomes.results.entries()) {
    const expected = expectedDenominator[index];
    const where = `exhaustive outcomes.results[${index}]`;
    assertExactKeys(
      result,
      ["audit", "durationMs", "error", "kind", "path", "status"],
      ["audit", "durationMs", "error", "kind", "path", "status"],
      where,
    );
    if (result.path !== expected.path || result.kind !== expected.kind) {
      fail(`${where} does not match the ordered artifact denominator`);
    }
    if (result.status !== "passed" || result.error !== null) {
      fail(`${where} is not a successful isolated artifact outcome`);
    }
    requireSafeInteger(result.durationMs, `${where}.durationMs`);
    assertExactKeys(
      result.audit,
      ["entries", "keySha256", "payloadSha256"],
      ["entries", "keySha256", "payloadSha256"],
      `${where}.audit`,
    );
    if (result.audit.entries !== expected.expectedEntries) {
      fail(`${where}.audit.entries differs from the catalog denominator`);
    }
    if (!SHA256.test(result.audit.keySha256) || !SHA256.test(result.audit.payloadSha256)) {
      fail(`${where}.audit logical SHA-256 is invalid`);
    }
    if (
      (expected.keySha256 !== null && expected.keySha256 !== result.audit.keySha256) ||
      (expected.payloadSha256 !== null && expected.payloadSha256 !== result.audit.payloadSha256)
    ) {
      fail(`${where}.audit differs from the logical baseline in the input catalog`);
    }
  }

  const expectedAuditText = canonicalAuditText(outcomes.results);
  if (auditText !== expectedAuditText) {
    fail("audit TSV is not the exact canonical complete-success outcomes projection");
  }
  validateIdentity(outcomes.audit, "exhaustive outcomes.audit");
  if (
    outcomes.audit.bytes !== Buffer.byteLength(auditText) ||
    outcomes.audit.sha256 !== sha256Text(auditText)
  ) {
    fail("exhaustive outcomes audit identity does not match the audit TSV bytes");
  }
  return {
    denominatorSha256,
    runner: outcomes.runner,
  };
}

export function recordLogicalBaselines(
  rawLock,
  auditText,
  outcomes,
  { catalogIdentity, outcomesIdentity } = {},
) {
  const lock = validateLock(structuredClone(rawLock));
  const artifacts = approvedArtifacts(lock);
  if (artifacts.length === 0) fail("lock has no approved artifacts");
  const evidence = validateExhaustiveEvidence({
    lock,
    auditText,
    outcomes,
    catalogIdentity,
    outcomesIdentity,
  });
  const audit = parseAudit(auditText);
  const expectedPaths = new Set(artifacts.map(({ artifact }) => artifact.path));
  for (const auditPath of audit.keys()) {
    if (!expectedPaths.has(auditPath)) fail(`audit contains unexpected path ${auditPath}`);
  }
  for (const { artifact } of artifacts) {
    const row = audit.get(artifact.path);
    if (!row) fail(`audit is missing path ${artifact.path}`);
    if (row.kind !== artifact.kind) {
      fail(`audit kind for ${artifact.path} is ${row.kind}; lock declares ${artifact.kind}`);
    }
    if (row.entries !== artifact.expectedEntries) {
      fail(`audit entries for ${artifact.path} is ${row.entries}; lock declares ${artifact.expectedEntries}`);
    }
    artifact.keySha256 = row.keySha256;
    artifact.payloadSha256 = row.payloadSha256;
    artifact.logicalDigestBasis = "mdictlib-self-observed";
    artifact.logicalObservation =
      `mdictlib isolated exhaustive audit (self-observed; not independent verification); ` +
      `catalog_sha256=${catalogIdentity.sha256}; ` +
      `denominator_sha256=${evidence.denominatorSha256}; ` +
      `runner_sha256=${evidence.runner.binarySha256}; ` +
      `runner_version=${evidence.runner.version}; ` +
      `protocol=${evidence.runner.protocol}; ` +
      `outcomes_sha256=${outcomesIdentity.sha256}; ` +
      `audit_sha256=${outcomes.audit.sha256}`;
  }
  return validateLock(lock);
}

export function validateLogicalBaselineChain(
  rawLock,
  auditText,
  outcomes,
  rawLogicalLock,
  { catalogIdentity, outcomesIdentity, logicalIdentity } = {},
) {
  validateIdentity(logicalIdentity, "logical lock identity");
  const expected = recordLogicalBaselines(rawLock, auditText, outcomes, {
    catalogIdentity,
    outcomesIdentity,
  });
  const actual = validateLock(structuredClone(rawLogicalLock));
  const expectedText = stableJson(expected);
  const actualText = stableJson(actual);
  if (
    logicalIdentity.bytes !== Buffer.byteLength(actualText) ||
    logicalIdentity.sha256 !== sha256Text(actualText)
  ) {
    fail("logical lock identity does not match its canonical JSON bytes");
  }
  if (actualText !== expectedText) {
    fail("logical lock is not the exact baseline derivation of the input catalog and exhaustive evidence");
  }
  return actual;
}

async function readJsonWithIdentity(inputPath, label) {
  const before = await sha256File(inputPath);
  const value = await readJson(inputPath);
  const after = await sha256File(inputPath);
  if (before.bytes !== after.bytes || before.sha256 !== after.sha256) {
    fail(`${label} changed while baseline evidence was being loaded`);
  }
  return { value, identity: before };
}

async function readTextWithIdentity(inputPath, label) {
  const before = await sha256File(inputPath);
  const value = await readFile(inputPath, "utf8");
  const after = await sha256File(inputPath);
  if (before.bytes !== after.bytes || before.sha256 !== after.sha256) {
    fail(`${label} changed while baseline evidence was being loaded`);
  }
  return { value, identity: before };
}

export async function main(argv = process.argv.slice(2)) {
  const options = parseOptions(argv, {
    "--catalog": "string",
    "--audit-tsv": "string",
    "--outcomes": "string",
    "--output": "string",
    "--accept-self-observed": "boolean",
    "--verify-chain": "boolean",
  });
  if (!options.catalog || !options["audit-tsv"] || !options.outcomes || !options.output) {
    fail("usage: record-logical-baselines.mjs --catalog <bootstrap.lock.json> --outcomes <exhaustive-outcomes.json> --audit-tsv <audit.tsv> --output <logical.lock.json> (--accept-self-observed | --verify-chain)");
  }
  if (options["verify-chain"] && options["accept-self-observed"]) {
    fail("--verify-chain and --accept-self-observed are mutually exclusive");
  }
  if (!options["verify-chain"] && !options["accept-self-observed"]) {
    fail("recording logical baselines requires --accept-self-observed");
  }
  await assertDistinctPaths({
    catalog: options.catalog,
    outcomes: options.outcomes,
    audit: options["audit-tsv"],
    output: options.output,
  });
  const [catalog, outcomes, audit, logical] = await Promise.all([
    readJsonWithIdentity(options.catalog, "catalog"),
    readJsonWithIdentity(options.outcomes, "exhaustive outcomes"),
    readTextWithIdentity(options["audit-tsv"], "audit TSV"),
    options["verify-chain"]
      ? readJsonWithIdentity(options.output, "logical lock")
      : Promise.resolve(null),
  ]);
  if (logical !== null) {
    validateLogicalBaselineChain(
      catalog.value,
      audit.value,
      outcomes.value,
      logical.value,
      {
        catalogIdentity: catalog.identity,
        outcomesIdentity: outcomes.identity,
        logicalIdentity: logical.identity,
      },
    );
    process.stdout.write(
      `Verified logical baseline chain: ${options.catalog} + ${options.outcomes} + ` +
        `${options["audit-tsv"]} -> ${options.output}\n`,
    );
    return;
  }
  const updated = recordLogicalBaselines(
    catalog.value,
    audit.value,
    outcomes.value,
    {
      catalogIdentity: catalog.identity,
      outcomesIdentity: outcomes.identity,
    },
  );
  await writeTextAtomic(options.output, stableJson(updated));
  process.stdout.write(
    `Recorded self-observed key and payload digests for ${approvedArtifacts(updated).length} artifacts in ${options.output}.\n` +
      "These are deterministic regression baselines, not independent parser correctness evidence.\n",
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`record-logical-baselines: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
