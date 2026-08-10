import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { AUDIT_PROTOCOL, CORPUS_AUDIT_TOOL, auditCorpus } from "./audit-corpus.mjs";
import { stableJson } from "./lib.mjs";
import { main as validateCorpusMain } from "./validate.mjs";

const FIXTURE_RUNNER = fileURLToPath(
  new URL("./test/fixtures/fake-audit-runner.mjs", import.meta.url),
);

async function temporaryRoot(label) {
  return mkdtemp(path.join(os.tmpdir(), `mdictlib-audit-${label}-`));
}

function review() {
  return {
    status: "approved",
    testingAllowed: true,
    redistributionAllowed: false,
    license: "unverified",
    licenseUrl: null,
    reviewedBy: "test reviewer",
    reviewedAt: "2026-08-10T00:00:00.000Z",
    notes: "Approved for a local synthetic subprocess test.",
  };
}

function artifact(relativePath, bytes) {
  return {
    kind: "mdx",
    sourcePath: relativePath,
    url: `https://example.test/${relativePath}`,
    resolvedUrl: `https://example.test/${relativePath}`,
    path: relativePath,
    bytes: bytes.length,
    sha256: createHash("sha256").update(bytes).digest("hex"),
    expectedEntries: 7,
    entryCountBasis: "mdictlib-self-observed",
    keySha256: null,
    payloadSha256: null,
    logicalDigestBasis: null,
    logicalObservation: null,
    observedEntries: 7,
    observation: "synthetic fixture",
    observationError: null,
    observer: {
      binaryBytes: 123,
      binarySha256: "c".repeat(64),
      mode: "metadata-open-and-count",
      timeoutMs: 1_000,
      tool: "synthetic test observer",
      version: "1",
    },
  };
}

async function fixture(root, names) {
  const artifacts = [];
  for (const [index, relativePath] of names.entries()) {
    const bytes = Buffer.from(`artifact-${index}-${relativePath}`);
    const target = path.join(root, ...relativePath.split("/"));
    await import("node:fs/promises").then(({ mkdir }) => mkdir(path.dirname(target), { recursive: true }));
    await writeFile(target, bytes);
    artifacts.push(artifact(relativePath, bytes));
  }
  const lock = {
    schemaVersion: 1,
    catalog: { name: "audit test", scope: "local synthetic artifacts" },
    entries: artifacts.map((lockedArtifact, index) => ({
      id: `artifact-${index}`,
      title: `Artifact ${index}`,
      infoUrl: `https://example.test/info/${index}`,
      review: review(),
      artifacts: [lockedArtifact],
    })),
  };
  const catalogPath = path.join(root, "catalog.lock.json");
  await writeFile(catalogPath, stableJson(lock));
  return { catalogPath, artifacts };
}

async function fakeRunner(identityPath = FIXTURE_RUNNER) {
  const bytes = await readFile(identityPath);
  return {
    command: process.execPath,
    argsPrefix: [identityPath],
    identityPath,
    identity: {
      binaryBytes: bytes.length,
      binarySha256: createHash("sha256").update(bytes).digest("hex"),
      protocol: AUDIT_PROTOCOL,
      tool: CORPUS_AUDIT_TOOL,
      version: "0.1.0-test",
    },
  };
}

test("isolated audit succeeds for collision-prone basenames and writes an exact atomic set", async (t) => {
  const root = await temporaryRoot("success");
  t.after(() => rm(root, { recursive: true, force: true }));
  const { catalogPath } = await fixture(root, ["a/same.mdx", "b/same.mdx"]);
  const outcomesPath = path.join(root, "outcomes.json");
  const auditOutputPath = path.join(root, "audit.tsv");

  const result = await auditCorpus({
    catalogPath,
    root,
    runner: await fakeRunner(),
    outcomesPath,
    auditOutputPath,
    concurrency: 2,
    timeoutMs: 2_000,
  });

  assert.equal(result.outcomes.completeSuccess, true);
  assert.deepEqual(
    result.outcomes.results.map(({ path: artifactPath, status }) => [artifactPath, status]),
    [
      ["a/same.mdx", "passed"],
      ["b/same.mdx", "passed"],
    ],
  );
  assert.equal(result.outcomes.denominator.artifactCount, 2);
  assert.equal(result.outcomes.schemaVersion, 2);
  assert.equal(result.outcomes.runner.protocol, AUDIT_PROTOCOL);
  assert.equal(result.outcomes.runner.tool, CORPUS_AUDIT_TOOL);
  assert.match(result.outcomes.runner.binarySha256, /^[0-9a-f]{64}$/);
  assert.deepEqual(Object.keys(result.outcomes.catalog).sort(), ["bytes", "sha256"]);
  const catalogBytes = await readFile(catalogPath);
  assert.equal(result.outcomes.catalog.bytes, catalogBytes.length);
  assert.equal(
    result.outcomes.catalog.sha256,
    createHash("sha256").update(catalogBytes).digest("hex"),
  );
  assert.equal(
    result.outcomes.denominator.sha256,
    createHash("sha256")
      .update(stableJson(result.outcomes.denominator.artifacts))
      .digest("hex"),
  );
  const persistedOutcomes = await readFile(outcomesPath, "utf8");
  assert.equal(JSON.parse(persistedOutcomes).results.length, 2);
  assert.doesNotMatch(persistedOutcomes, new RegExp(root.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
  assert.doesNotMatch(
    persistedOutcomes,
    new RegExp(process.cwd().replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
  );
  assert.equal((await readFile(auditOutputPath, "utf8")).split("\n").length, 4);
  assert.deepEqual(
    (await readdir(root)).filter((name) => name.includes(".part-")),
    [],
  );
});

test("runner failure and output overflow do not prevent later artifact outcomes", async (t) => {
  const root = await temporaryRoot("continuation");
  t.after(() => rm(root, { recursive: true, force: true }));
  const { catalogPath } = await fixture(root, ["fail.mdx", "good.mdx", "spam.mdx"]);
  const outcomesPath = path.join(root, "outcomes.json");
  const auditOutputPath = path.join(root, "audit.tsv");
  await writeFile(outcomesPath, "stale outcomes\n");
  await writeFile(auditOutputPath, "stale\n");

  const result = await auditCorpus({
    catalogPath,
    root,
    runner: await fakeRunner(),
    outcomesPath,
    auditOutputPath,
    concurrency: 1,
    timeoutMs: 2_000,
  });

  assert.equal(result.outcomes.completeSuccess, false);
  assert.deepEqual(
    result.outcomes.results.map(({ path: artifactPath, status }) => [artifactPath, status]),
    [
      ["fail.mdx", "failed"],
      ["good.mdx", "passed"],
      ["spam.mdx", "failed"],
    ],
  );
  assert.equal(result.outcomes.results[0].error.type, "runner");
  assert.ok(result.outcomes.results[0].error.message.length <= 2_049);
  assert.doesNotMatch(result.outcomes.results[0].error.message, /[\r\n\t\0]/);
  assert.doesNotMatch(result.outcomes.results[0].error.message, new RegExp(root.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
  assert.doesNotMatch(
    result.outcomes.results[0].error.message,
    new RegExp(process.cwd().replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
  );
  assert.match(result.outcomes.results[0].error.message, /<corpus-artifact>|<workspace>/);
  assert.equal(result.outcomes.results[2].error.type, "output-limit");
  assert.equal(existsSync(auditOutputPath), false);
  assert.equal(JSON.parse(await readFile(outcomesPath, "utf8")).results.length, 3);
});

test("runner replacement invalidates every result and suppresses the combined audit", async (t) => {
  const root = await temporaryRoot("runner-replacement");
  const runnerRoot = await temporaryRoot("external-runner");
  t.after(async () => {
    await rm(root, { recursive: true, force: true });
    await rm(runnerRoot, { recursive: true, force: true });
  });
  const { catalogPath } = await fixture(root, ["replace.mdx", "good.mdx"]);
  const runnerPath = path.join(runnerRoot, "mutable-runner.mjs");
  await writeFile(runnerPath, await readFile(FIXTURE_RUNNER));
  const outcomesPath = path.join(root, "outcomes.json");
  const auditOutputPath = path.join(root, "audit.tsv");

  const result = await auditCorpus({
    catalogPath,
    root,
    runner: await fakeRunner(runnerPath),
    outcomesPath,
    auditOutputPath,
    concurrency: 1,
    timeoutMs: 2_000,
  });

  assert.equal(result.outcomes.completeSuccess, false);
  assert.equal(result.outcomes.summary.passed, 0);
  assert.ok(result.outcomes.results.every(({ error }) => error.type === "runner-identity"));
  assert.equal(JSON.stringify(result.outcomes).includes(runnerRoot), false);
  assert.equal(existsSync(auditOutputPath), false);
  assert.equal(result.outcomes.audit, null);
});

test("full-mode build failure cannot leave stale outcomes or audit TSV", async (t) => {
  const root = await temporaryRoot("build-failure");
  t.after(() => rm(root, { recursive: true, force: true }));
  const { catalogPath } = await fixture(root, ["good.mdx"]);
  const outcomesPath = path.join(root, "outcomes.json");
  const auditOutputPath = path.join(root, "audit.tsv");
  await writeFile(outcomesPath, "stale outcomes\n");
  await writeFile(auditOutputPath, "stale audit\n");

  await assert.rejects(
    validateCorpusMain([
      "--catalog",
      catalogPath,
      "--root",
      root,
      "--mode",
      "full",
      "--cargo",
      process.execPath,
      "--outcomes-output",
      outcomesPath,
      "--audit-output",
      auditOutputPath,
    ]),
    /exited with status/,
  );
  assert.equal(existsSync(outcomesPath), false);
  assert.equal(existsSync(auditOutputPath), false);
});

test("a timed-out artifact is killed without suppressing sibling success", async (t) => {
  const root = await temporaryRoot("timeout");
  t.after(() => rm(root, { recursive: true, force: true }));
  const { catalogPath } = await fixture(root, ["good.mdx", "hang.mdx"]);
  const outcomesPath = path.join(root, "outcomes.json");

  const result = await auditCorpus({
    catalogPath,
    root,
    runner: await fakeRunner(),
    outcomesPath,
    concurrency: 2,
    timeoutMs: 100,
  });

  assert.equal(result.outcomes.completeSuccess, false);
  assert.equal(result.outcomes.results.find(({ path: value }) => value === "good.mdx").status, "passed");
  assert.equal(result.outcomes.results.find(({ path: value }) => value === "hang.mdx").error.type, "timeout");
});

test("audit outputs cannot collide with each other or overwrite locked bytes", async (t) => {
  const root = await temporaryRoot("outputs");
  t.after(() => rm(root, { recursive: true, force: true }));
  const { catalogPath, artifacts } = await fixture(root, ["protected.mdx"]);
  const protectedPath = path.join(root, artifacts[0].path);
  const original = await readFile(protectedPath);

  await assert.rejects(
    auditCorpus({
      catalogPath,
      root,
      runner: await fakeRunner(),
      outcomesPath: protectedPath,
      concurrency: 1,
      timeoutMs: 2_000,
    }),
    /must not alias (?:artifact 0|outcomes output)/,
  );
  assert.deepEqual(await readFile(protectedPath), original);

  const sameOutput = path.join(root, "same-output");
  await assert.rejects(
    auditCorpus({
      catalogPath,
      root,
      runner: await fakeRunner(),
      outcomesPath: sameOutput,
      auditOutputPath: sameOutput,
      concurrency: 1,
      timeoutMs: 2_000,
    }),
    /must not alias outcomes output/,
  );

  const ownershipLock = path.join(root, ".protected.mdx.part.lock");
  await writeFile(ownershipLock, "owned\n");
  await assert.rejects(
    auditCorpus({
      catalogPath,
      root,
      runner: await fakeRunner(),
      outcomesPath: ownershipLock,
      concurrency: 1,
      timeoutMs: 2_000,
    }),
    /must not alias (?:artifact partial ownership 0|outcomes output)/,
  );
});
