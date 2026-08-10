#!/usr/bin/env node

import { rm } from "node:fs/promises";
import { homedir } from "node:os";
import path from "node:path";
import process from "node:process";
import { spawn, spawnSync } from "node:child_process";
import {
  MANIFEST_NAME,
  approvedArtifacts,
  assertExactKeys,
  assertDistinctPaths,
  fail,
  mapBounded,
  readJson,
  requireSafeInteger,
  requireString,
  resolveCorpusPath,
  sha256File,
  sha256Text,
  stableJson,
  validateLock,
  verifyArtifact,
  writeTextAtomic,
} from "./lib.mjs";

export const AUDIT_PROTOCOL = "mdictlib-corpus-audit-v1";
export const OUTCOMES_SCHEMA_VERSION = 2;
export const CORPUS_AUDIT_TOOL = "mdictlib corpus_audit";
const IDENTITY_PROTOCOL = "mdictlib-corpus-audit-identity-v1";
const AUDIT_HEADER = "path\tkind\tentries\tkey_sha256\tpayload_sha256";
const SHA256 = /^[0-9a-f]{64}$/;
const MAX_CAPTURE_BYTES = 64 * 1024;
const MAX_DIAGNOSTIC_CHARS = 2_048;

function denominatorArtifact(entry, artifact) {
  return {
    entryId: entry.id,
    path: artifact.path,
    kind: artifact.kind,
    bytes: artifact.bytes,
    sha256: artifact.sha256,
    expectedEntries: artifact.expectedEntries,
    keySha256: artifact.keySha256,
    payloadSha256: artifact.payloadSha256,
  };
}

export function exhaustiveDenominator(lock) {
  return approvedArtifacts(lock).map(({ entry, artifact }) => denominatorArtifact(entry, artifact));
}

function sanitizeDiagnostic(value) {
  const characters = [];
  let truncated = false;
  for (const character of String(value)) {
    if (characters.length >= MAX_DIAGNOSTIC_CHARS) {
      truncated = true;
      break;
    }
    if (character === "\n" || character === "\r" || character === "\t") {
      characters.push(" ");
    } else if (/\p{Cc}/u.test(character)) {
      characters.push("�");
    } else {
      characters.push(character);
    }
  }
  return `${characters.join("")}${truncated ? "…" : ""}`;
}

function redactLocalPaths(value, { root, target = null, runnerPath = null }) {
  let redacted = String(value);
  const replacements = [
    [target, "<corpus-artifact>"],
    [runnerPath, "<runner>"],
    [root, "<corpus>"],
    [process.cwd(), "<workspace>"],
    [homedir(), "<home>"],
  ]
    .filter(([candidate]) => typeof candidate === "string" && candidate.length > 1)
    .sort(([left], [right]) => right.length - left.length);
  for (const [candidate, replacement] of replacements) {
    redacted = redacted.replaceAll(candidate, replacement);
  }
  return redacted;
}

function failure(type, message, paths) {
  return { type, message: sanitizeDiagnostic(redactLocalPaths(message, paths)) };
}

function appendCapture(chunks, currentBytes, chunk) {
  const bytes = Buffer.from(chunk);
  const remaining = Math.max(0, MAX_CAPTURE_BYTES - currentBytes);
  if (remaining > 0) chunks.push(bytes.subarray(0, remaining));
  return {
    bytes: currentBytes + Math.min(bytes.length, remaining),
    exceeded: bytes.length > remaining,
  };
}

function protocolError(message, type = "protocol") {
  const error = new Error(message);
  error.auditFailureType = type;
  return error;
}

function validateRunner(runner) {
  if (!runner || typeof runner.command !== "string" || runner.command === "") {
    fail("audit runner command is required");
  }
  if (!Array.isArray(runner.argsPrefix) || runner.argsPrefix.some((value) => typeof value !== "string")) {
    fail("audit runner argsPrefix must be an array of strings");
  }
  if (typeof runner.identityPath !== "string" || runner.identityPath === "") {
    fail("audit runner identityPath is required");
  }
  assertExactKeys(
    runner.identity,
    ["binaryBytes", "binarySha256", "protocol", "tool", "version"],
    ["binaryBytes", "binarySha256", "protocol", "tool", "version"],
    "audit runner identity",
  );
  requireSafeInteger(runner.identity.binaryBytes, "audit runner identity.binaryBytes", 1);
  if (!SHA256.test(runner.identity.binarySha256)) {
    fail("audit runner identity.binarySha256 must be 64 lowercase hexadecimal digits");
  }
  if (runner.identity.protocol !== AUDIT_PROTOCOL) {
    fail(`audit runner identity.protocol must be ${AUDIT_PROTOCOL}`);
  }
  requireString(runner.identity.tool, "audit runner identity.tool");
  requireString(runner.identity.version, "audit runner identity.version");
  return runner;
}

async function runnerIdentityError(runner) {
  try {
    const actual = await sha256File(runner.identityPath);
    if (
      actual.bytes !== runner.identity.binaryBytes ||
      actual.sha256 !== runner.identity.binarySha256
    ) {
      return "audit runner binary identity differs from the built executable";
    }
    return null;
  } catch (error) {
    return `failed to verify audit runner binary identity: ${error instanceof Error ? error.message : String(error)}`;
  }
}

function runArtifactProcess({ runner, artifact, target, timeoutMs }) {
  return new Promise((resolve) => {
    const started = Date.now();
    let stdoutBytes = 0;
    let stderrBytes = 0;
    let stdoutExceeded = false;
    let stderrExceeded = false;
    const stdout = [];
    const stderr = [];
    let timedOut = false;
    let spawnError = null;
    let settled = false;
    const child = spawn(
      runner.command,
      [...(runner.argsPrefix ?? []), artifact.kind, target, String(artifact.expectedEntries)],
      {
        env: { ...process.env, RUST_BACKTRACE: "0" },
        shell: false,
        stdio: ["ignore", "pipe", "pipe"],
      },
    );

    const finish = (code, signal) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve({
        code,
        signal,
        durationMs: Date.now() - started,
        timedOut,
        spawnError,
        outputExceeded: stdoutExceeded || stderrExceeded,
        stdout: Buffer.concat(stdout).toString("utf8"),
        stderr: Buffer.concat(stderr).toString("utf8"),
      });
    };

    child.stdout.on("data", (chunk) => {
      const capture = appendCapture(stdout, stdoutBytes, chunk);
      stdoutBytes = capture.bytes;
      stdoutExceeded ||= capture.exceeded;
      if (capture.exceeded) child.kill("SIGKILL");
    });
    child.stderr.on("data", (chunk) => {
      const capture = appendCapture(stderr, stderrBytes, chunk);
      stderrBytes = capture.bytes;
      stderrExceeded ||= capture.exceeded;
      if (capture.exceeded) child.kill("SIGKILL");
    });
    child.on("error", (error) => {
      spawnError = error;
      finish(null, null);
    });
    child.on("close", finish);
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill("SIGKILL");
    }, timeoutMs);
    timer.unref();
  });
}

function parseSuccess(stdout, artifact) {
  const lines = stdout.split(/\r?\n/);
  if (lines.at(-1) === "") lines.pop();
  if (lines.length !== 1) throw protocolError("runner stdout must contain exactly one protocol row");
  const fields = lines[0].split("\t");
  if (fields.length !== 5 || fields[0] !== AUDIT_PROTOCOL) {
    throw protocolError(`runner stdout is not ${AUDIT_PROTOCOL}`);
  }
  const [, kind, entriesText, keySha256, payloadSha256] = fields;
  if (kind !== artifact.kind) throw protocolError(`runner reported kind ${JSON.stringify(kind)}`);
  if (!/^[0-9]+$/.test(entriesText) || entriesText !== String(artifact.expectedEntries)) {
    throw protocolError(`runner reported entry count ${JSON.stringify(entriesText)}`);
  }
  if (!SHA256.test(keySha256) || !SHA256.test(payloadSha256)) {
    throw protocolError("runner reported an invalid logical SHA-256");
  }
  if (artifact.keySha256 !== null && artifact.keySha256 !== keySha256) {
    throw protocolError(`key SHA-256 differs from the reviewed lock: ${keySha256}`, "baseline");
  }
  if (artifact.payloadSha256 !== null && artifact.payloadSha256 !== payloadSha256) {
    throw protocolError(
      `payload SHA-256 differs from the reviewed lock: ${payloadSha256}`,
      "baseline",
    );
  }
  return { entries: artifact.expectedEntries, keySha256, payloadSha256 };
}

async function auditOne({ row, root, runner, timeoutMs }) {
  const { artifact } = row;
  const started = Date.now();
  let target;
  try {
    target = await verifyArtifact(root, artifact);
  } catch (error) {
    return {
      path: artifact.path,
      kind: artifact.kind,
      status: "failed",
      durationMs: Date.now() - started,
      audit: null,
      error: failure("identity", error instanceof Error ? error.message : String(error), {
        root,
        target,
      }),
    };
  }

  const runnerBeforeError = await runnerIdentityError(runner);
  if (runnerBeforeError !== null) {
    return {
      path: artifact.path,
      kind: artifact.kind,
      status: "failed",
      durationMs: Date.now() - started,
      audit: null,
      error: failure("runner-identity", runnerBeforeError, {
        root,
        target,
        runnerPath: runner.identityPath,
      }),
    };
  }
  const processResult = await runArtifactProcess({ runner, artifact, target, timeoutMs });
  const runnerAfterError = await runnerIdentityError(runner);
  if (runnerAfterError !== null) {
    return {
      path: artifact.path,
      kind: artifact.kind,
      status: "failed",
      durationMs: Date.now() - started,
      audit: null,
      error: failure("runner-identity", runnerAfterError, {
        root,
        target,
        runnerPath: runner.identityPath,
      }),
    };
  }
  try {
    await verifyArtifact(root, artifact);
  } catch (error) {
    return {
      path: artifact.path,
      kind: artifact.kind,
      status: "failed",
      durationMs: Date.now() - started,
      audit: null,
      error: failure(
        "identity",
        `locked identity changed during its audit: ${error instanceof Error ? error.message : String(error)}`,
        { root, target },
      ),
    };
  }
  let error = null;
  let audit = null;
  if (processResult.timedOut) {
    error = failure("timeout", `artifact audit exceeded ${timeoutMs} ms`, { root, target });
  } else if (processResult.outputExceeded) {
    error = failure("output-limit", `artifact audit exceeded the ${MAX_CAPTURE_BYTES}-byte output limit`, {
      root,
      target,
    });
  } else if (processResult.spawnError !== null) {
    error = failure("spawn", "failed to start the artifact audit runner", { root, target });
  } else if (processResult.code !== 0) {
    const detail = processResult.stderr.trim() || `runner exited with status ${processResult.code ?? "unknown"}`;
    error = failure("runner", detail, { root, target });
  } else {
    try {
      audit = parseSuccess(processResult.stdout, artifact);
    } catch (parseError) {
      error = failure(
        parseError?.auditFailureType === "baseline" ? "baseline" : "protocol",
        parseError instanceof Error ? parseError.message : String(parseError),
        { root, target },
      );
    }
  }
  return {
    path: artifact.path,
    kind: artifact.kind,
    status: error === null ? "passed" : "failed",
    durationMs: Date.now() - started,
    audit,
    error,
  };
}

async function assertOutputSafety({ catalogPath, root, rows, outcomesPath, auditOutputPath }) {
  const outputs = {
    "outcomes output": outcomesPath,
    "audit output": auditOutputPath,
  };
  await assertDistinctPaths(outputs);
  const protectedPaths = {
    catalog: catalogPath,
    manifest: path.resolve(root, MANIFEST_NAME),
  };
  for (const [index, { artifact }] of rows.entries()) {
    const target = resolveCorpusPath(root, artifact.path);
    const partial = path.join(path.dirname(target), `.${path.basename(target)}.part`);
    protectedPaths[`artifact ${index}`] = target;
    protectedPaths[`artifact journal ${index}`] = `${target}.mdictlib-lock.json`;
    protectedPaths[`artifact partial ${index}`] = partial;
    protectedPaths[`artifact partial metadata ${index}`] = `${partial}.json`;
    protectedPaths[`artifact partial ownership ${index}`] = `${partial}.lock`;
  }
  for (const [name, protectedPath] of Object.entries(protectedPaths)) {
    await assertDistinctPaths({ ...outputs, [name]: protectedPath });
  }
}

async function loadCatalogIdentity(catalogPath) {
  const before = await sha256File(catalogPath);
  const lock = validateLock(await readJson(catalogPath));
  const after = await sha256File(catalogPath);
  if (before.bytes !== after.bytes || before.sha256 !== after.sha256) {
    fail("catalog changed while its exhaustive audit denominator was being loaded");
  }
  return { lock, identity: before };
}

async function catalogIdentityError(catalogPath, expected) {
  try {
    const actual = await sha256File(catalogPath);
    return actual.bytes === expected.bytes && actual.sha256 === expected.sha256
      ? null
      : "catalog identity changed after the exhaustive denominator was prepared";
  } catch (error) {
    return `failed to reverify catalog identity: ${error instanceof Error ? error.message : String(error)}`;
  }
}

async function assertRunnerSafety(prepared, runner, catalogPath) {
  const fixed = {
    "runner binary": runner.identityPath,
    "outcomes output": prepared.outcomesPath,
    "audit output": prepared.auditOutputPath,
  };
  const protectedPaths = {
    catalog: catalogPath,
    manifest: path.resolve(prepared.root, MANIFEST_NAME),
  };
  for (const [index, { artifact }] of prepared.rows.entries()) {
    protectedPaths[`artifact ${index}`] = resolveCorpusPath(prepared.root, artifact.path);
  }
  for (const [name, protectedPath] of Object.entries(protectedPaths)) {
    await assertDistinctPaths({ ...fixed, [name]: protectedPath });
  }
}

export async function prepareAuditRun({
  catalogPath,
  root,
  outcomesPath,
  auditOutputPath = null,
}) {
  if (!outcomesPath) fail("audit outcomes path is required");
  const { lock, identity } = await loadCatalogIdentity(catalogPath);
  const rows = approvedArtifacts(lock);
  if (rows.length === 0) fail("reviewed lock has no approved local-testing dictionary artifacts");
  const resolvedRoot = path.resolve(root);
  const resolvedOutcomes = path.resolve(outcomesPath);
  const resolvedAudit = auditOutputPath === null ? null : path.resolve(auditOutputPath);
  await assertOutputSafety({
    catalogPath,
    root: resolvedRoot,
    rows,
    outcomesPath: resolvedOutcomes,
    auditOutputPath: resolvedAudit,
  });
  await Promise.all([
    rm(resolvedOutcomes, { force: true }),
    resolvedAudit === null ? Promise.resolve() : rm(resolvedAudit, { force: true }),
  ]);
  return {
    catalogIdentity: identity,
    lock,
    rows,
    root: resolvedRoot,
    outcomesPath: resolvedOutcomes,
    auditOutputPath: resolvedAudit,
  };
}

function auditText(results) {
  return `${[
    AUDIT_HEADER,
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

export async function auditCorpus({
  catalogPath,
  root,
  runner,
  outcomesPath,
  auditOutputPath = null,
  prepared = null,
  concurrency = 2,
  timeoutMs = 3_600_000,
  onProgress = null,
}) {
  if (!Number.isSafeInteger(concurrency) || concurrency < 1) fail("audit concurrency must be a positive safe integer");
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1) fail("artifact timeout must be a positive safe integer");
  if (!outcomesPath) fail("audit outcomes path is required");
  validateRunner(runner);
  const run =
    prepared ??
    (await prepareAuditRun({ catalogPath, root, outcomesPath, auditOutputPath }));
  if (
    run.root !== path.resolve(root) ||
    run.outcomesPath !== path.resolve(outcomesPath) ||
    run.auditOutputPath !== (auditOutputPath === null ? null : path.resolve(auditOutputPath))
  ) {
    fail("prepared audit paths do not match the requested run");
  }
  await assertRunnerSafety(run, runner, catalogPath);
  const catalogBeforeError = await catalogIdentityError(catalogPath, run.catalogIdentity);
  const runnerBeforeError = await runnerIdentityError(runner);
  const denominator = exhaustiveDenominator(run.lock);
  let completed = 0;
  let results;
  const preparationError = catalogBeforeError ?? runnerBeforeError;
  if (preparationError !== null) {
    const type = catalogBeforeError !== null ? "catalog-identity" : "runner-identity";
    results = run.rows.map(({ artifact }) => ({
      path: artifact.path,
      kind: artifact.kind,
      status: "failed",
      durationMs: 0,
      audit: null,
      error: failure(type, preparationError, {
        root: run.root,
        runnerPath: runner.identityPath,
      }),
    }));
  } else {
    results = await mapBounded(run.rows, concurrency, async (row) => {
    const started = Date.now();
    let result;
    try {
      result = await auditOne({ row, root: run.root, runner, timeoutMs });
    } catch (error) {
      result = {
        path: row.artifact.path,
        kind: row.artifact.kind,
        status: "failed",
        durationMs: Date.now() - started,
        audit: null,
        error: failure(
          "orchestrator",
          error instanceof Error ? error.message : String(error),
          {
            root: run.root,
            target: resolveCorpusPath(run.root, row.artifact.path),
          },
        ),
      };
    }
    completed += 1;
    if (onProgress !== null) {
      try {
        onProgress({ completed, total: run.rows.length, result });
      } catch {
        // Progress reporting is non-authoritative and must not truncate the exact outcome set.
      }
    }
    return result;
  });
  }
  const [catalogAfterError, runnerAfterError] = await Promise.all([
    catalogIdentityError(catalogPath, run.catalogIdentity),
    runnerIdentityError(runner),
  ]);
  const finalIdentityError = catalogAfterError ?? runnerAfterError;
  if (finalIdentityError !== null) {
    const type = catalogAfterError !== null ? "catalog-identity" : "runner-identity";
    results = results.map((result) => ({
      ...result,
      status: "failed",
      audit: null,
      error: failure(type, finalIdentityError, {
        root: run.root,
        runnerPath: runner.identityPath,
      }),
    }));
  }
  const passed = results.filter((result) => result.status === "passed").length;
  const failed = results.length - passed;
  const completeSuccess = results.length === run.rows.length && failed === 0;
  const outcomes = {
    schemaVersion: OUTCOMES_SCHEMA_VERSION,
    generatedAt: new Date().toISOString(),
    protocol: AUDIT_PROTOCOL,
    catalog: {
      bytes: run.catalogIdentity.bytes,
      sha256: run.catalogIdentity.sha256,
    },
    runner: structuredClone(runner.identity),
    denominator: {
      artifactCount: denominator.length,
      sha256: sha256Text(stableJson(denominator)),
      artifacts: denominator,
    },
    execution: { concurrency, artifactTimeoutMs: timeoutMs },
    completeSuccess,
    summary: { passed, failed },
    results,
  };
  let combinedAudit = completeSuccess ? auditText(results) : null;
  outcomes.audit = combinedAudit === null
    ? null
    : {
        bytes: Buffer.byteLength(combinedAudit),
        sha256: sha256Text(combinedAudit),
      };
  const outcomesText = stableJson(outcomes);
  const outcomesIdentity = {
    bytes: Buffer.byteLength(outcomesText),
    sha256: sha256Text(outcomesText),
  };
  await writeTextAtomic(run.outcomesPath, outcomesText);
  if (combinedAudit !== null && run.auditOutputPath !== null) {
    await writeTextAtomic(run.auditOutputPath, combinedAudit);
  }
  return {
    lock: run.lock,
    rows: run.rows,
    catalogIdentity: run.catalogIdentity,
    outcomes,
    outcomesIdentity,
    auditText: combinedAudit,
  };
}

function queryBuiltRunnerIdentity(executable, env) {
  const result = spawnSync(executable, ["--identity"], {
    encoding: "utf8",
    env: { ...env, RUST_BACKTRACE: "0" },
    maxBuffer: 16 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
  });
  if (result.error) fail(`failed to query the built corpus_audit identity: ${result.error.message}`);
  if (result.status !== 0) {
    fail(`built corpus_audit identity query exited with status ${result.status}`);
  }
  const lines = result.stdout.split(/\r?\n/);
  if (lines.at(-1) === "") lines.pop();
  if (lines.length !== 1) fail("built corpus_audit identity output must contain exactly one row");
  const [identityProtocol, protocol, tool, version, ...extra] = lines[0].split("\t");
  if (
    identityProtocol !== IDENTITY_PROTOCOL ||
    protocol !== AUDIT_PROTOCOL ||
    tool !== CORPUS_AUDIT_TOOL ||
    typeof version !== "string" ||
    version === "" ||
    extra.length !== 0
  ) {
    fail("built corpus_audit reported an unexpected versioned identity");
  }
  return { protocol, tool, version };
}

export async function buildAuditRunner({ cargo = "cargo", env = process.env } = {}) {
  const args = [
    "build",
    "--locked",
    "--release",
    "--all-features",
    "--example",
    "corpus_audit",
    "--message-format=json-render-diagnostics",
  ];
  const result = spawnSync(cargo, args, {
    encoding: "utf8",
    env,
    maxBuffer: 64 * 1024 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
  });
  if (result.error) fail(`failed to run ${cargo}: ${result.error.message}`);
  if (result.status !== 0) {
    const diagnostics = result.stdout
      .split(/\r?\n/)
      .filter(Boolean)
      .map((line) => {
        try {
          return JSON.parse(line).message?.rendered ?? "";
        } catch {
          return "";
        }
      })
      .join("");
    fail(`${cargo} ${args.join(" ")} exited with status ${result.status}: ${sanitizeDiagnostic(diagnostics || result.stderr)}`);
  }
  let executable = null;
  for (const line of result.stdout.split(/\r?\n/)) {
    if (line === "") continue;
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      continue;
    }
    if (
      message.reason === "compiler-artifact" &&
      message.target?.name === "corpus_audit" &&
      message.target?.kind?.includes("example") &&
      typeof message.executable === "string"
    ) {
      executable = message.executable;
    }
  }
  if (executable === null) fail("cargo did not report the corpus_audit example executable");
  const before = await sha256File(executable);
  const reported = queryBuiltRunnerIdentity(executable, env);
  const after = await sha256File(executable);
  if (before.bytes !== after.bytes || before.sha256 !== after.sha256) {
    fail("built corpus_audit executable changed while its identity was being recorded");
  }
  return {
    command: executable,
    argsPrefix: [],
    identityPath: executable,
    identity: {
      binaryBytes: before.bytes,
      binarySha256: before.sha256,
      ...reported,
    },
  };
}
