import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { chmod, mkdir, mkdtemp, readFile, rm, stat, symlink, writeFile } from "node:fs/promises";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { importAalookupCatalog } from "../import-aalookup-catalog.mjs";
import {
  AUDIT_PROTOCOL,
  CORPUS_AUDIT_TOOL,
  OUTCOMES_SCHEMA_VERSION,
  exhaustiveDenominator,
} from "../audit-corpus.mjs";
import {
  inspectEntries,
  inspectVerifiedEntries,
  main as lockCorpusMain,
} from "../lock-corpus.mjs";
import {
  assertDistinctPaths,
  downloadAtomic,
  manifestText,
  requireAcquisitionUrl,
  requireSameOriginRedirect,
  sanitizeDiagnostic,
  selectionArtifactSetSha256,
  sha256File,
  sourceRowSetSha256,
  stableJson,
  validateLock,
  validateSelection,
  verifyArtifact,
} from "../lib.mjs";
import { selectInventory, validateSelectionAgainstInventory } from "../select-inventory.mjs";
import {
  main as promoteLockMain,
  promoteDraft,
  validatePromotionPair,
} from "../promote-lock.mjs";
import {
  main as recordLogicalBaselinesMain,
  recordLogicalBaselines,
  validateLogicalBaselineChain,
} from "../record-logical-baselines.mjs";
import { syncCorpus } from "../sync.mjs";

const PAYLOAD = Buffer.from("deterministic-mdict-corpus-payload");
const SHA256 = createHash("sha256").update(PAYLOAD).digest("hex");
const TEST_NETWORK_POLICY = { allowInsecureHttp: true, allowPrivateAddresses: true };
const OBSERVER = {
  binaryBytes: 123,
  binarySha256: "c".repeat(64),
  mode: "metadata-open-and-count",
  timeoutMs: 5_000,
  tool: "test observer --count-only",
  version: "0.1.0-test",
};

async function temporaryRoot(label) {
  return mkdtemp(path.join(os.tmpdir(), `mdictlib-${label}-`));
}

async function waitForCondition(predicate, { timeoutMs = 5_000, intervalMs = 10 } = {}) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, intervalMs));
  }
  throw new Error(`condition not met within ${timeoutMs} ms`);
}

async function serverFixture() {
  let truncationRequests = 0;
  let totalRequests = 0;
  let resolveOversizeDripClosed;
  const oversizeDripClosed = new Promise((resolve) => {
    resolveOversizeDripClosed = resolve;
  });
  const server = http.createServer((request, response) => {
    totalRequests += 1;
    if (request.url === "/redirect") {
      response.writeHead(302, { location: "/file" });
      response.end();
      return;
    }
    if (request.url === "/cross-origin") {
      response.writeHead(302, { location: `http://localhost:${server.address().port}/file` });
      response.end();
      return;
    }
    if (request.url === "/query-redirect") {
      response.writeHead(302, { location: "/file?token=super-secret" });
      response.end();
      return;
    }
    if (request.url === "/drip") {
      response.writeHead(200, { "content-length": 128 });
      const interval = setInterval(() => response.write(Buffer.from([0x61])), 10);
      response.on("close", () => clearInterval(interval));
      return;
    }
    if (request.url === "/slow-file") {
      response.writeHead(200, { "content-length": PAYLOAD.length, etag: '"fixture-v1"' });
      response.flushHeaders?.();
      response.write(PAYLOAD.subarray(0, 11));
      setTimeout(() => response.end(PAYLOAD.subarray(11)), 2_000);
      return;
    }
    if (request.url === "/truncate") {
      truncationRequests += 1;
      const range = request.headers.range;
      if (range) {
        const offset = Number(range.match(/^bytes=([0-9]+)-$/)?.[1]);
        assert.equal(request.headers["if-range"], '"fixture-v1"');
        response.writeHead(206, {
          "accept-ranges": "bytes",
          "content-length": PAYLOAD.length - offset,
          "content-range": `bytes ${offset}-${PAYLOAD.length - 1}/${PAYLOAD.length}`,
          etag: '"fixture-v1"',
        });
        response.end(PAYLOAD.subarray(offset));
      } else {
        response.writeHead(200, {
          "accept-ranges": "bytes",
          "content-length": PAYLOAD.length,
          etag: '"fixture-v1"',
        });
        response.flushHeaders?.();
        response.write(PAYLOAD.subarray(0, 11));
        setTimeout(() => response.destroy(), 500);
      }
      return;
    }
    if (request.url === "/file" || request.url === "/wrong") {
      response.writeHead(200, { "content-length": PAYLOAD.length, etag: '"fixture-v1"' });
      response.end(PAYLOAD);
      return;
    }
    if (request.url === "/oversize") {
      response.writeHead(200, { "content-length": PAYLOAD.length });
      response.end(PAYLOAD);
      return;
    }
    if (request.url === "/oversize-drip") {
      response.writeHead(200, { "content-length": 128 });
      response.flushHeaders();
      const interval = setInterval(() => response.write(Buffer.from([0x61])), 10);
      response.on("close", () => {
        clearInterval(interval);
        resolveOversizeDripClosed();
      });
      return;
    }
    response.writeHead(404);
    response.end();
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  return {
    base: `http://127.0.0.1:${address.port}`,
    close: () => new Promise((resolve, reject) => {
      server.close((error) => (error ? reject(error) : resolve()));
      server.closeAllConnections?.();
    }),
    oversizeDripClosed,
    truncationRequests: () => truncationRequests,
    totalRequests: () => totalRequests,
  };
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
    notes: "Explicitly approved for private local parser testing only.",
  };
}

function lockedArtifact(base, relativePath, urlPath = "/file") {
  return {
    kind: "mdx",
    sourcePath: `source/${relativePath}`,
    url: `${base}${urlPath}`,
    resolvedUrl: `${base}${urlPath === "/redirect" ? "/file" : urlPath}`,
    path: relativePath,
    bytes: PAYLOAD.length,
    sha256: SHA256,
    expectedEntries: 7,
    entryCountBasis: "mdictlib-self-observed",
    keySha256: null,
    payloadSha256: null,
    logicalDigestBasis: null,
    logicalObservation: null,
    observedEntries: 7,
    observation: "test snapshot",
    observationError: null,
    observer: OBSERVER,
  };
}

function lockFor(base, artifacts) {
  return {
    schemaVersion: 1,
    catalog: { name: "test", scope: "local HTTP fixture only" },
    entries: artifacts.map((artifact, index) => ({
      id: `fixture-${index}`,
      title: `Fixture ${index}`,
      infoUrl: `${base}/info/${index}`,
      review: review(),
      artifacts: [artifact],
    })),
  };
}

function exhaustiveEvidence(lock, auditText) {
  const denominator = exhaustiveDenominator(lock);
  const byPath = new Map(
    auditText
      .trim()
      .split("\n")
      .slice(1)
      .map((line) => {
        const [artifactPath, kind, entries, keySha256, payloadSha256] = line.split("\t");
        return [artifactPath, { entries: Number(entries), keySha256, kind, payloadSha256 }];
      }),
  );
  const results = denominator.map((artifact) => ({
    audit: {
      entries: byPath.get(artifact.path).entries,
      keySha256: byPath.get(artifact.path).keySha256,
      payloadSha256: byPath.get(artifact.path).payloadSha256,
    },
    durationMs: 1,
    error: null,
    kind: artifact.kind,
    path: artifact.path,
    status: "passed",
  }));
  const catalogText = stableJson(lock);
  const catalogIdentity = {
    bytes: Buffer.byteLength(catalogText),
    sha256: createHash("sha256").update(catalogText).digest("hex"),
  };
  const outcomes = {
    audit: {
      bytes: Buffer.byteLength(auditText),
      sha256: createHash("sha256").update(auditText).digest("hex"),
    },
    catalog: catalogIdentity,
    completeSuccess: true,
    denominator: {
      artifactCount: denominator.length,
      artifacts: denominator,
      sha256: createHash("sha256").update(stableJson(denominator)).digest("hex"),
    },
    execution: { artifactTimeoutMs: 1_000, concurrency: 1 },
    generatedAt: "2026-08-10T00:00:00.000Z",
    protocol: AUDIT_PROTOCOL,
    results,
    runner: {
      binaryBytes: 123,
      binarySha256: "d".repeat(64),
      protocol: AUDIT_PROTOCOL,
      tool: CORPUS_AUDIT_TOOL,
      version: "0.1.0-test",
    },
    schemaVersion: OUTCOMES_SCHEMA_VERSION,
    summary: { failed: 0, passed: denominator.length },
  };
  const outcomesText = stableJson(outcomes);
  return {
    catalogIdentity,
    outcomes,
    outcomesIdentity: {
      bytes: Buffer.byteLength(outcomesText),
      sha256: createHash("sha256").update(outcomesText).digest("hex"),
    },
  };
}

function addAcquisitionOutcomes(draft) {
  const rows = draft.entries.flatMap((entry) =>
    entry.artifacts.map((artifact) => ({
      acquisition: "downloaded",
      advertisedBytes: artifact.bytes,
      bytes: artifact.bytes,
      entryId: entry.id,
      error: null,
      infoUrl: entry.infoUrl,
      kind: artifact.kind,
      path: artifact.path,
      sourcePath: artifact.sourcePath,
      resolvedUrl: artifact.resolvedUrl,
      review: entry.review,
      sha256: artifact.sha256,
      sourceTitle: entry.title,
      status: "acquired",
      url: artifact.url,
    })),
  );
  draft.acquisitionOutcomes = rows;
  const denominator = rows.map((row) => ({ ...row, advertisedBytes: row.advertisedBytes }));
  const sourceRows = denominator.map((row) => ({
    advertisedBytes: row.advertisedBytes,
    kind: row.kind,
    sourcePath: row.sourcePath,
    url: row.url,
  }));
  draft.selectionBinding = {
    advertisedBytes: rows.reduce((sum, row) => sum + row.advertisedBytes, 0),
    artifactCount: rows.length,
    artifactSetSha256: selectionArtifactSetSha256(denominator),
    entryCount: draft.entries.length,
    selectionSha256: "b".repeat(64),
    source: {
      inventorySha256: "a".repeat(64),
      kind: "mdict-index-inventory-v1",
      root: new URL("/", rows[0].url).toString(),
      selectedAdvertisedBytes: rows.reduce((sum, row) => sum + row.advertisedBytes, 0),
      selectedCount: rows.length,
      selectedSetSha256: sourceRowSetSha256(sourceRows),
      selectedType: "mdx",
      snapshotAt: "2026-08-10T00:00:00.000Z",
    },
  };
  draft.selectionBinding.selectionSha256 = createHash("sha256").update(stableJson({
    catalog: draft.catalog,
    entries: rows.map((outcome) => ({
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
    source: draft.selectionBinding.source,
  })).digest("hex");
  return draft;
}

function inventoryFixture(base, routes) {
  const files = routes.map((route, index) => ({
    bytes: PAYLOAD.length,
    parent: "source",
    path: `source/${index}-${route}.mdx`,
    type: "mdx",
    url: `${base}/${route}`,
  }));
  return {
    advertisedBytes: files.reduce((sum, file) => sum + file.bytes, 0),
    fileCount: files.length,
    files,
    pageCount: 1,
    root: `${base}/`,
    schemaVersion: 1,
    snapshotAt: "2026-08-10T00:00:00.000Z",
  };
}

function reviewedSelection(inventory) {
  const inventoryText = stableJson(inventory);
  const inventorySha256 = createHash("sha256").update(inventoryText).digest("hex");
  return {
    inventorySha256,
    inventoryText,
    selection: selectInventory(inventory, {
      inventorySha256,
      networkPolicy: TEST_NETWORK_POLICY,
      notes: "Private local testing requested by the maintainer.",
      reviewedAt: "2026-08-10T00:00:00.000Z",
      reviewedBy: "maintainer",
      type: "mdx",
    }),
  };
}

test("AALookup import classifies every URL and globally deduplicates deterministically", () => {
  const draft = [
    {
      title: "One",
      type: "mdx",
      url: "https://example.test/info#fragment",
      downloadable: true,
      downloadUrls: [
        "https://example.test/files/one.MDX",
        "https://example.test/files/one.MDX#duplicate",
        "https://example.test/files/assets.zip",
      ],
    },
    {
      title: "Two",
      type: "collection",
      url: "https://example.test/info",
      downloadable: false,
      downloadUrls: [],
    },
  ];
  const first = importAalookupCatalog(draft, "a".repeat(64));
  const second = importAalookupCatalog(structuredClone(draft), "a".repeat(64));
  assert.equal(stableJson(first), stableJson(second));
  assert.equal(first.urls.length, 3);
  assert.deepEqual(
    first.urls.map(({ classification }) => classification).sort(),
    ["archive", "mdx", "other"],
  );
  const info = first.urls.find(({ url }) => url === "https://example.test/info");
  assert.deepEqual(info.roles, ["info"]);
  assert.equal(info.references.length, 2);
  const mdx = first.urls.find(({ classification }) => classification === "mdx");
  assert.equal(mdx.references.length, 1);
});

test("inventory selection requires explicit review data and preserves full paths", () => {
  const inventory = {
    schemaVersion: 1,
    root: "https://mdx.example.test/",
    snapshotAt: "2026-08-10T00:00:00.000Z",
    pageCount: 1,
    fileCount: 2,
    advertisedBytes: 15,
    files: [
      { path: "中文/A.MDX", type: "mdx", bytes: 10, url: "https://mdx.example.test/%E4%B8%AD%E6%96%87/A.MDX", parent: "中文" },
      { path: "other/readme.txt", type: "txt", bytes: 5, url: "https://mdx.example.test/other/readme.txt", parent: "other" },
    ],
  };
  const options = {
    inventorySha256: "a".repeat(64),
    type: "mdx",
    reviewedBy: "maintainer",
    reviewedAt: "2026-08-10T00:00:00.000Z",
    notes: "Private local testing requested by the maintainer.",
  };
  const first = selectInventory(inventory, options);
  const second = selectInventory(structuredClone(inventory), options);
  assert.equal(stableJson(first), stableJson(second));
  assert.match(first.entries[0].artifacts[0].path, /^mdict-org\/mdx\/[0-9a-f]{32}\.mdx$/);
  assert.equal(first.entries[0].artifacts[0].advertisedBytes, 10);
  assert.equal(first.entries[0].title, "中文/A.MDX");
  assert.equal(first.entries[0].review.redistributionAllowed, false);
  assert.equal(first.entries[0].review.license, "unverified");
  const unsafe = structuredClone(first);
  unsafe.entries[0].review.redistributionAllowed = true;
  assert.throws(() => validateSelection(unsafe), /affirmative license evidence/);
  const crossOrigin = structuredClone(inventory);
  crossOrigin.files[0].url = "https://attacker.example/A.MDX";
  assert.throws(() => selectInventory(crossOrigin, options), /inventory origin/);
});

test("reviewed selection is bound to every matching row in the exact inventory bytes", () => {
  const inventory = {
    advertisedBytes: 30,
    fileCount: 2,
    files: [
      { bytes: 10, parent: "all", path: "all/a.mdx", type: "mdx", url: "https://public.example/all/a.mdx" },
      { bytes: 20, parent: "all", path: "all/b.mdx", type: "mdx", url: "https://public.example/all/b.mdx" },
    ],
    pageCount: 1,
    root: "https://public.example/",
    schemaVersion: 1,
    snapshotAt: "2026-08-10T00:00:00.000Z",
  };
  const inventorySha256 = createHash("sha256").update(stableJson(inventory)).digest("hex");
  const selection = selectInventory(inventory, {
    inventorySha256,
    notes: "Exact inventory denominator review.",
    reviewedAt: "2026-08-10T00:00:00.000Z",
    reviewedBy: "maintainer",
    type: "mdx",
  });
  validateSelectionAgainstInventory(selection, inventory, inventorySha256);

  const omitted = structuredClone(selection);
  omitted.entries.splice(1, 1);
  const remaining = omitted.entries[0].artifacts[0];
  omitted.source.selectedCount = 1;
  omitted.source.selectedAdvertisedBytes = remaining.advertisedBytes;
  omitted.source.selectedSetSha256 = sourceRowSetSha256([remaining]);
  validateSelection(omitted);
  assert.throws(
    () => validateSelectionAgainstInventory(omitted, inventory, inventorySha256),
    /inventory requires 2|source facts do not match/,
  );
});

test("streamed download follows redirects and commits only the verified file", async (t) => {
  const root = await temporaryRoot("redirect");
  const fixture = await serverFixture();
  t.after(async () => {
    await fixture.close();
    await rm(root, { recursive: true, force: true });
  });
  const destination = path.join(root, "nested", "redirect.mdx");
  const result = await downloadAtomic({
    url: `${fixture.base}/redirect`,
    destination,
    root,
    maxBytes: 1024,
    expectedBytes: PAYLOAD.length,
    expectedSha256: SHA256,
    expectedResolvedUrl: `${fixture.base}/file`,
    timeoutMs: 5_000,
    networkPolicy: TEST_NETWORK_POLICY,
  });
  assert.equal(result.resolvedUrl, `${fixture.base}/file`);
  assert.deepEqual(await readFile(destination), PAYLOAD);
  assert.equal(existsSync(path.join(root, "nested", ".redirect.mdx.part")), false);
  assert.equal(existsSync(path.join(root, "nested", ".redirect.mdx.part.json")), false);
});

test("acquisition URLs and redirects fail closed without retaining secrets", async (t) => {
  assert.throws(
    () => requireAcquisitionUrl("http://public.example/file.mdx", "candidate"),
    /must use HTTPS/,
  );
  const secretUrl = "https://public.example/file.mdx?token=do-not-log";
  assert.throws(
    () => requireAcquisitionUrl(secretUrl, "candidate"),
    (error) => /query string/.test(error.message) && !error.message.includes("do-not-log"),
  );
  for (const blocked of [
    "https://127.0.0.1/file.mdx",
    "https://169.254.169.254/latest/meta-data",
    "https://192.31.196.1/file.mdx",
    "https://[64:ff9b:1::7f00:1]/file.mdx",
    "https://[5f00::1]/file.mdx",
    "https://[fec0::1]/file.mdx",
    "https://[2001:db8::1]/file.mdx",
    "https://[2d00::1]/file.mdx",
    "https://[3000::1]/file.mdx",
    "https://[3ffe::1]/file.mdx",
    "https://[3fff::1]/file.mdx",
    "https://metadata.google.internal/computeMetadata/v1/",
  ]) {
    assert.throws(() => requireAcquisitionUrl(blocked, "candidate"), /non-public|metadata|fully-qualified/);
  }
  assert.equal(
    requireAcquisitionUrl("https://[2606:4700:4700::1111]/file.mdx", "candidate"),
    "https://[2606:4700:4700::1111]/file.mdx",
  );
  assert.throws(
    () => requireSameOriginRedirect(
      "https://other.example/file.mdx",
      "https://public.example/start.mdx",
      "https://public.example/start.mdx",
    ),
    /reviewed URL origin/,
  );
  assert.throws(
    () => requireSameOriginRedirect(
      "http://public.example/file.mdx",
      "https://public.example/start.mdx",
      "https://public.example/start.mdx",
    ),
    /must use HTTPS/,
  );

  const root = await temporaryRoot("redirect-policy");
  const fixture = await serverFixture();
  t.after(async () => {
    await fixture.close();
    await rm(root, { recursive: true, force: true });
  });
  for (const route of ["cross-origin", "query-redirect"]) {
    await assert.rejects(
      downloadAtomic({
        url: `${fixture.base}/${route}`,
        destination: path.join(root, `${route}.mdx`),
        root,
        maxBytes: 1024,
        timeoutMs: 1_000,
        networkPolicy: TEST_NETWORK_POLICY,
      }),
      route === "cross-origin" ? /reviewed URL origin/ : /query string/,
    );
  }
});

test("absolute download deadline stops a progress drip that defeats inactivity timeout", async (t) => {
  const root = await temporaryRoot("absolute-deadline");
  const fixture = await serverFixture();
  t.after(async () => {
    await fixture.close();
    await rm(root, { recursive: true, force: true });
  });
  const started = Date.now();
  await assert.rejects(
    downloadAtomic({
      url: `${fixture.base}/drip`,
      destination: path.join(root, "drip.mdx"),
      root,
      maxBytes: 1024,
      timeoutMs: 100,
      deadlineMs: 60,
      networkPolicy: TEST_NETWORK_POLICY,
    }),
    /deadline|aborted|fetch/i,
  );
  assert.ok(Date.now() - started < 2_000);
});

test("truncated response leaves a stable partial and the next run resumes with Range", async (t) => {
  const root = await temporaryRoot("resume");
  const fixture = await serverFixture();
  t.after(async () => {
    await fixture.close();
    await rm(root, { recursive: true, force: true });
  });
  const destination = path.join(root, "resume.mdx");
  const options = {
    url: `${fixture.base}/truncate`,
    destination,
    root,
    maxBytes: 1024,
    expectedBytes: PAYLOAD.length,
    expectedSha256: SHA256,
    expectedResolvedUrl: `${fixture.base}/truncate`,
    timeoutMs: 5_000,
    networkPolicy: TEST_NETWORK_POLICY,
  };
  await assert.rejects(downloadAtomic(options), /aborted|terminated|truncated|fetch|network activity/i);
  assert.equal(existsSync(destination), false);
  assert.equal(existsSync(path.join(root, ".resume.mdx.part")), true);
  assert.equal(existsSync(path.join(root, ".resume.mdx.part.json")), true);
  await downloadAtomic(options);
  assert.deepEqual(await readFile(destination), PAYLOAD);
  assert.equal(fixture.truncationRequests(), 2);
  assert.equal(existsSync(path.join(root, ".resume.mdx.part")), false);
  assert.equal(existsSync(path.join(root, ".resume.mdx.part.json")), false);
});

test("a concurrent partial mutation is never installed or journaled as verified bytes", async (t) => {
  const root = await temporaryRoot("partial-mutation");
  const fixture = await serverFixture();
  const destination = path.join(root, "mutated.mdx");
  const partial = path.join(root, ".mutated.mdx.part");
  const downloading = downloadAtomic({
    url: `${fixture.base}/slow-file`,
    destination,
    root,
    maxBytes: 1024,
    expectedBytes: PAYLOAD.length,
    expectedSha256: SHA256,
    expectedResolvedUrl: `${fixture.base}/slow-file`,
    timeoutMs: 5_000,
    networkPolicy: TEST_NETWORK_POLICY,
  });
  t.after(async () => {
    await downloading.catch(() => {});
    await fixture.close();
    await rm(root, { recursive: true, force: true });
  });
  await waitForCondition(async () => existsSync(partial) && (await stat(partial)).size >= 11);
  await writeFile(partial, Buffer.alloc(PAYLOAD.length, 0x78));
  await assert.rejects(downloading, /completed partial bytes differ/);
  assert.equal(existsSync(destination), false);
  assert.equal(existsSync(partial), false);
  assert.equal(existsSync(`${partial}.json`), false);
  assert.equal(existsSync(`${partial}.lock`), false);
});

test("diagnostics redact configured local roots while retaining useful relative context", () => {
  const workspace = path.join(os.tmpdir(), "private-workspace");
  const corpusRoot = path.join(workspace, ".corpus");
  const target = path.join(corpusRoot, "mdict-org", "mdx", "artifact.mdx");
  const observer = path.join(os.tmpdir(), "private-observer", "inspect");
  const diagnostic = sanitizeDiagnostic(`failed to run ${observer} on ${target} from ${workspace}`, {
    workspaceRoot: workspace,
    corpusRoot,
    target,
    observerPath: observer,
    homeRoot: null,
  });
  assert.equal(
    diagnostic,
    "failed to run <observer> on <corpus-artifact> from <workspace>",
  );
  assert.equal(diagnostic.includes(workspace), false);
  assert.equal(diagnostic.includes(observer), false);
});

test("wrong hash and oversize responses never replace the final path and clean unusable partials", async (t) => {
  const root = await temporaryRoot("rejected");
  const fixture = await serverFixture();
  t.after(async () => {
    await fixture.close();
    await rm(root, { recursive: true, force: true });
  });
  for (const [name, options, pattern] of [
    ["wrong.mdx", { expectedBytes: PAYLOAD.length, expectedSha256: "0".repeat(64), maxBytes: 1024 }, /SHA-256/],
    ["oversize.mdx", { expectedBytes: null, expectedSha256: null, maxBytes: 4 }, /file limit/],
  ]) {
    const destination = path.join(root, name);
    await assert.rejects(
      downloadAtomic({
        url: `${fixture.base}/${name.startsWith("wrong") ? "wrong" : "oversize"}`,
        destination,
        root,
        expectedResolvedUrl: null,
        timeoutMs: 5_000,
        networkPolicy: TEST_NETWORK_POLICY,
        ...options,
      }),
      pattern,
    );
    assert.equal(existsSync(destination), false);
    assert.equal(existsSync(path.join(root, `.${name}.part`)), false);
    assert.equal(existsSync(path.join(root, `.${name}.part.json`)), false);
  }
});

test("header rejection cancels a never-ending response body", async (t) => {
  const root = await temporaryRoot("rejected-stream");
  const fixture = await serverFixture();
  t.after(async () => {
    await fixture.close();
    await rm(root, { recursive: true, force: true });
  });
  await assert.rejects(
    downloadAtomic({
      url: `${fixture.base}/oversize-drip`,
      destination: path.join(root, "oversize-drip.mdx"),
      root,
      maxBytes: 4,
      timeoutMs: 5_000,
      deadlineMs: 5_000,
      networkPolicy: TEST_NETWORK_POLICY,
    }),
    /file limit/,
  );
  await Promise.race([
    fixture.oversizeDripClosed,
    new Promise((_, reject) => setTimeout(() => reject(new Error("response body was not cancelled")), 1_000)),
  ]);
});

test("lock validation rejects traversal before any destination can be resolved", () => {
  const lock = lockFor("https://example.test", [
    { ...lockedArtifact("https://example.test", "safe.mdx"), path: "../escape.mdx" },
  ]);
  assert.throws(() => validateLock(lock), /normalized relative POSIX path/);
});

test("committed lock schema mirrors runtime artifact and review invariants", async () => {
  const schema = JSON.parse(await readFile(new URL("../../../corpus/catalog.schema.json", import.meta.url), "utf8"));
  const committedLock = JSON.parse(await readFile(new URL("../../../corpus/catalog.lock.json", import.meta.url), "utf8"));
  validateLock(committedLock);
  const valid = lockFor("https://example.test", [lockedArtifact("https://example.test", "UPPER.MDX")]);
  validateLock(valid);
  const mismatchedCount = structuredClone(valid);
  mismatchedCount.entries[0].artifacts[0].expectedEntries += 1;
  assert.throws(() => validateLock(mismatchedCount), /must equal observedEntries/);
  assert.deepEqual(
    Object.keys(valid.entries[0].artifacts[0]).sort(),
    [...schema.$defs.artifact.required].sort(),
  );
  assert.equal(new RegExp(schema.$defs.artifact.properties.path.pattern).test("UPPER.MDX"), true);
  assert.equal(new RegExp(schema.$defs.nullableNonemptyString.anyOf[0].pattern).test("   "), false);
  const redistributionLicense = schema.$defs.review.allOf[1].then.properties.license.pattern;
  assert.equal(new RegExp(redistributionLicense).test(" unverified "), false);
  assert.equal(new RegExp(redistributionLicense).test("MIT"), true);
});

test("promotion requires successful observations and retains failures in outcomes", () => {
  const base = "https://example.test";
  const success = lockedArtifact(base, "safe/success.mdx");
  const failed = lockedArtifact(base, "safe/failed.mdx");
  const draft = addAcquisitionOutcomes(lockFor(base, [success, failed]));
  for (const entry of draft.entries) {
    for (const artifact of entry.artifacts) {
      artifact.expectedEntries = null;
      artifact.entryCountBasis = null;
      artifact.keySha256 = null;
      artifact.payloadSha256 = null;
      artifact.logicalDigestBasis = null;
      artifact.logicalObservation = null;
    }
  }
  draft.entries[1].artifacts[0].observedEntries = null;
  draft.entries[1].artifacts[0].observation = null;
  draft.entries[1].artifacts[0].observationError = "unsupported fixture";
  const { lock, outcomes } = promoteDraft(draft);
  validatePromotionPair(lock, outcomes);
  assert.equal(lock.entries.length, 1);
  assert.equal(lock.entries[0].artifacts[0].expectedEntries, 7);
  assert.equal(lock.entries[0].artifacts[0].entryCountBasis, "mdictlib-self-observed");
  assert.deepEqual(outcomes.results.map(({ status }) => status), ["promoted", "excluded"]);
  assert.equal(outcomes.results[1].observationError, "unsupported fixture");
  assert.equal(outcomes.results[1].url, `${base}/file`);
  assert.equal(outcomes.results[1].resolvedUrl, `${base}/file`);
  const tamperedLock = structuredClone(lock);
  tamperedLock.entries[0].artifacts[0].sha256 = "f".repeat(64);
  assert.throws(
    () => validatePromotionPair(tamperedLock, outcomes),
    /lock bytes do not match|differs from its complete outcome/,
  );
  const tamperedOutcomes = structuredClone(outcomes);
  tamperedOutcomes.results[0].status = "excluded";
  assert.throws(
    () => validatePromotionPair(lock, tamperedOutcomes),
    /excluded entry-count|review facts differ|non-promoted outcome/,
  );
  const missingObserver = structuredClone(outcomes);
  delete missingObserver.results[0].observer;
  assert.throws(
    () => validatePromotionPair(lock, missingObserver),
    /missing field "observer"/,
  );
  const changedObserver = structuredClone(outcomes);
  changedObserver.results[0].observer.binarySha256 = "e".repeat(64);
  assert.throws(
    () => validatePromotionPair(lock, changedObserver),
    /differs from its complete outcome/,
  );
  const fabricatedLogicalLock = structuredClone(lock);
  const fabricatedArtifact = fabricatedLogicalLock.entries[0].artifacts[0];
  fabricatedArtifact.keySha256 = "1".repeat(64);
  fabricatedArtifact.payloadSha256 = "2".repeat(64);
  fabricatedArtifact.logicalDigestBasis = "mdictlib-self-observed";
  fabricatedArtifact.logicalObservation = "fabricated logical provenance";
  const fabricatedOutcomes = structuredClone(outcomes);
  const fabricatedText = stableJson(fabricatedLogicalLock);
  fabricatedOutcomes.promotedLock = {
    bytes: Buffer.byteLength(fabricatedText),
    sha256: createHash("sha256").update(fabricatedText).digest("hex"),
  };
  assert.throws(
    () => validatePromotionPair(fabricatedLogicalLock, fabricatedOutcomes),
    /contains logical baselines/,
  );
});

test("promotion rejects a failed row removed from both visible draft arrays", () => {
  const base = "https://example.test";
  const draft = addAcquisitionOutcomes(lockFor(base, [
    lockedArtifact(base, "kept.mdx"),
    lockedArtifact(base, "failed.mdx"),
  ]));
  for (const entry of draft.entries) {
    const artifact = entry.artifacts[0];
    artifact.expectedEntries = null;
    artifact.entryCountBasis = null;
  }
  draft.entries.splice(1, 1);
  Object.assign(draft.acquisitionOutcomes[1], {
    acquisition: null,
    bytes: null,
    error: "source unavailable",
    resolvedUrl: null,
    sha256: null,
    status: "acquisition-error",
  });
  assert.equal(promoteDraft(draft).outcomes.results[1].status, "acquisition-error");
  const truncated = structuredClone(draft);
  truncated.acquisitionOutcomes.splice(1, 1);
  assert.throws(() => promoteDraft(truncated), /bound selection requires 2/);
});

test("promotion rejects authorization metadata changed after selection binding", () => {
  const draft = addAcquisitionOutcomes(lockFor("https://example.test", [
    lockedArtifact("https://example.test", "reviewed.mdx"),
  ]));
  const artifact = draft.entries[0].artifacts[0];
  artifact.expectedEntries = null;
  artifact.entryCountBasis = null;
  draft.entries[0].review.notes = "Tampered after review.";
  draft.acquisitionOutcomes[0].review.notes = "Tampered after review.";
  assert.throws(() => promoteDraft(draft), /exact bound canonical selection bytes/);
});

test("promotion CLI emits and verifies a cryptographically paired lock and complete outcomes", async (t) => {
  const root = await temporaryRoot("promotion-pair");
  t.after(() => rm(root, { recursive: true, force: true }));
  const draft = addAcquisitionOutcomes(lockFor("https://example.test", [
    lockedArtifact("https://example.test", "paired.mdx"),
  ]));
  const artifact = draft.entries[0].artifacts[0];
  artifact.expectedEntries = null;
  artifact.entryCountBasis = null;
  artifact.keySha256 = null;
  artifact.payloadSha256 = null;
  artifact.logicalDigestBasis = null;
  artifact.logicalObservation = null;
  const input = path.join(root, "draft.json");
  const output = path.join(root, "lock.json");
  const outcomes = path.join(root, "outcomes.json");
  await writeFile(input, stableJson(draft));
  await promoteLockMain([
    "--input", input,
    "--output", output,
    "--outcomes", outcomes,
    "--accept-self-observed",
  ]);
  await promoteLockMain([
    "--verify-pair",
    "--output", output,
    "--outcomes", outcomes,
  ]);
  const recorded = JSON.parse(await readFile(outcomes, "utf8"));
  assert.equal(recorded.sourceDraftSha256, createHash("sha256").update(await readFile(input)).digest("hex"));
  assert.equal(recorded.promotedLock.sha256, createHash("sha256").update(await readFile(output)).digest("hex"));

  const tampered = JSON.parse(await readFile(output, "utf8"));
  tampered.catalog.scope = "tampered but still structurally valid";
  await writeFile(output, stableJson(tampered));
  await assert.rejects(
    promoteLockMain(["--verify-pair", "--output", output, "--outcomes", outcomes]),
    /catalog differs|lock bytes do not match/,
  );
});

test("metadata-only observer is argument-bound, output-capped, and time-bounded", async (t) => {
  const root = await temporaryRoot("observer");
  t.after(() => rm(root, { recursive: true, force: true }));
  const checkingObserver = path.join(root, "checking-observer.mjs");
  await writeFile(
    checkingObserver,
    "if (process.argv.at(-1) !== '--count-only') process.exit(4); process.stdout.write('entries=9\\n');\n",
  );
  assert.deepEqual(
    inspectEntries([process.execPath, checkingObserver], { kind: "mdx", destination: "ignored.mdx" }, 1_000),
    { entries: 9, error: null },
  );

  const hangingObserver = path.join(root, "hanging-observer.mjs");
  await writeFile(hangingObserver, "setInterval(() => {}, 1000);\n");
  const started = Date.now();
  const timedOut = inspectEntries(
    [process.execPath, hangingObserver],
    { kind: "mdx", destination: "ignored.mdx" },
    50,
  );
  assert.equal(timedOut.entries, null);
  assert.match(timedOut.error, /exceeded 50 ms/);
  assert.ok(Date.now() - started < 2_000);

  const artifactPath = path.join(root, "observed.mdx");
  const mutatingObserver = path.join(root, "mutating-observer.mjs");
  await writeFile(artifactPath, PAYLOAD);
  await writeFile(
    mutatingObserver,
    "import { statSync, writeFileSync } from 'node:fs'; const p = process.argv.at(-2); writeFileSync(p, Buffer.alloc(statSync(p).size, 0x78)); process.stdout.write('entries=9\\n');\n",
  );
  const nodeIdentity = await sha256File(process.execPath);
  await assert.rejects(
    inspectVerifiedEntries(
      [process.execPath, mutatingObserver],
      {
        bytes: PAYLOAD.length,
        destination: artifactPath,
        kind: "mdx",
        path: "observed.mdx",
        sha256: SHA256,
      },
      {
        root,
        timeoutMs: 1_000,
        observer: {
          binaryBytes: nodeIdentity.bytes,
          binarySha256: nodeIdentity.sha256,
        },
      },
    ),
    /SHA-256/,
  );
});

test("logical baseline recording requires an exact, duplicate-free artifact set", () => {
  const base = "https://example.test";
  const lock = lockFor(base, [
    lockedArtifact(base, "b/b.mdx"),
    lockedArtifact(base, "a/a.mdx"),
  ]);
  const keyA = "1".repeat(64);
  const payloadA = "2".repeat(64);
  const keyB = "3".repeat(64);
  const payloadB = "4".repeat(64);
  const audit =
    `path\tkind\tentries\tkey_sha256\tpayload_sha256\n` +
    `a/a.mdx\tmdx\t7\t${keyA}\t${payloadA}\n` +
    `b/b.mdx\tmdx\t7\t${keyB}\t${payloadB}\n`;
  const evidence = exhaustiveEvidence(lock, audit);
  const record = (auditText, outcomes = evidence.outcomes) =>
    recordLogicalBaselines(lock, auditText, outcomes, {
      catalogIdentity: evidence.catalogIdentity,
      outcomesIdentity: evidence.outcomesIdentity,
    });
  const updated = record(audit);
  assert.equal(updated.entries[0].artifacts[0].keySha256, keyB);
  assert.equal(updated.entries[1].artifacts[0].payloadSha256, payloadA);
  assert.equal(updated.entries[0].artifacts[0].logicalDigestBasis, "mdictlib-self-observed");
  assert.match(updated.entries[0].artifacts[0].logicalObservation, /runner_sha256=d{64}/);
  const logicalText = stableJson(updated);
  assert.deepEqual(
    validateLogicalBaselineChain(lock, audit, evidence.outcomes, updated, {
      catalogIdentity: evidence.catalogIdentity,
      outcomesIdentity: evidence.outcomesIdentity,
      logicalIdentity: {
        bytes: Buffer.byteLength(logicalText),
        sha256: createHash("sha256").update(logicalText).digest("hex"),
      },
    }),
    updated,
  );
  const fabricated = structuredClone(updated);
  fabricated.entries[0].artifacts[0].keySha256 = "f".repeat(64);
  const fabricatedText = stableJson(fabricated);
  assert.throws(
    () => validateLogicalBaselineChain(lock, audit, evidence.outcomes, fabricated, {
      catalogIdentity: evidence.catalogIdentity,
      outcomesIdentity: evidence.outcomesIdentity,
      logicalIdentity: {
        bytes: Buffer.byteLength(fabricatedText),
        sha256: createHash("sha256").update(fabricatedText).digest("hex"),
      },
    }),
    /exact baseline derivation/,
  );
  assert.throws(
    () => record(`${audit}a/a.mdx\tmdx\t7\t${keyA}\t${payloadA}\n`),
    /canonical complete-success outcomes projection/,
  );
  assert.throws(
    () => record(audit.replace("a/a.mdx\tmdx\t7", "a/a.mdx\tmdx\t8")),
    /canonical complete-success outcomes projection/,
  );
  assert.throws(
    () => record(audit.split("\n").filter((line) => !line.startsWith("b/b.mdx")).join("\n")),
    /canonical complete-success outcomes projection/,
  );
  const incomplete = structuredClone(evidence.outcomes);
  incomplete.completeSuccess = false;
  assert.throws(() => record(audit, incomplete), /completeSuccess=true/);
});

test("logical baseline CLI verifies the exact chain and rejects stale inputs or aliases", async (t) => {
  const root = await temporaryRoot("logical-evidence");
  t.after(() => rm(root, { recursive: true, force: true }));
  const lock = lockFor("https://example.test", [
    lockedArtifact("https://example.test", "one.mdx"),
  ]);
  const key = "1".repeat(64);
  const payload = "2".repeat(64);
  const audit =
    `path\tkind\tentries\tkey_sha256\tpayload_sha256\n` +
    `one.mdx\tmdx\t7\t${key}\t${payload}\n`;
  const evidence = exhaustiveEvidence(lock, audit);
  const catalogPath = path.join(root, "catalog.json");
  const outcomesPath = path.join(root, "outcomes.json");
  const auditPath = path.join(root, "audit.tsv");
  const outputPath = path.join(root, "logical.json");
  await writeFile(catalogPath, stableJson(lock));
  await writeFile(outcomesPath, stableJson(evidence.outcomes));
  await writeFile(auditPath, audit);
  const args = [
    "--catalog", catalogPath,
    "--outcomes", outcomesPath,
    "--audit-tsv", auditPath,
    "--output", outputPath,
    "--accept-self-observed",
  ];

  await recordLogicalBaselinesMain(args);
  assert.equal(JSON.parse(await readFile(outputPath, "utf8")).entries[0].artifacts[0].keySha256, key);
  const verifyArgs = [...args.slice(0, -1), "--verify-chain"];
  await recordLogicalBaselinesMain(verifyArgs);

  const fabricatedLogical = JSON.parse(await readFile(outputPath, "utf8"));
  fabricatedLogical.entries[0].artifacts[0].keySha256 = "f".repeat(64);
  await writeFile(outputPath, stableJson(fabricatedLogical));
  await assert.rejects(recordLogicalBaselinesMain(verifyArgs), /exact baseline derivation/);
  await recordLogicalBaselinesMain(args);

  const staleCatalog = structuredClone(lock);
  staleCatalog.catalog.scope = "changed after the exhaustive run";
  await writeFile(catalogPath, stableJson(staleCatalog));
  await assert.rejects(recordLogicalBaselinesMain(args), /catalog identity does not match/);
  await writeFile(catalogPath, stableJson(lock));

  await writeFile(auditPath, audit.replace(key, "3".repeat(64)));
  await assert.rejects(recordLogicalBaselinesMain(args), /canonical complete-success|audit identity/);
  await writeFile(auditPath, audit);

  const staleOutcomes = structuredClone(evidence.outcomes);
  staleOutcomes.catalog.sha256 = "0".repeat(64);
  await writeFile(outcomesPath, stableJson(staleOutcomes));
  await assert.rejects(recordLogicalBaselinesMain(args), /catalog identity does not match/);
  await writeFile(outcomesPath, stableJson(evidence.outcomes));

  await assert.rejects(
    recordLogicalBaselinesMain(args.map((value) => (value === outputPath ? outcomesPath : value))),
    /must not alias outcomes/,
  );
});

test("bootstrap preflights advertised bytes, writes inspect failures, and reuses journals", async (t) => {
  const root = await temporaryRoot("bootstrap");
  const fixture = await serverFixture();
  t.after(async () => {
    await fixture.close();
    await rm(root, { recursive: true, force: true });
  });
  const inventory = inventoryFixture(fixture.base, ["file"]);
  const { inventoryText, selection } = reviewedSelection(inventory);
  const inventoryPath = path.join(root, "inventory.json");
  const selectionPath = path.join(root, "selection.json");
  await writeFile(inventoryPath, inventoryText);
  await writeFile(selectionPath, stableJson(selection));
  const before = fixture.totalRequests();
  await assert.rejects(
    lockCorpusMain([
      "--selection", selectionPath,
      "--inventory", inventoryPath,
      "--output", path.join(root, "too-large.json"),
      "--root", path.join(root, "bytes"),
      "--max-total-bytes", String(PAYLOAD.length - 1),
    ], { networkPolicy: TEST_NETWORK_POLICY }),
    /advertises .* exceeding/,
  );
  assert.equal(fixture.totalRequests(), before);

  const corpusRoot = path.join(root, "bytes");
  const draftPath = path.join(root, "draft.json");
  const fakeCargo = path.join(root, process.platform === "win32" ? "fake-cargo.cmd" : "fake-cargo");
  await writeFile(fakeCargo, process.platform === "win32" ? "@exit /b 2\r\n" : "#!/bin/sh\nexit 2\n");
  if (process.platform !== "win32") await chmod(fakeCargo, 0o700);
  await lockCorpusMain([
    "--selection", selectionPath,
    "--inventory", inventoryPath,
    "--output", draftPath,
    "--root", corpusRoot,
    "--cargo", fakeCargo,
    "--retries", "0",
    "--timeout-ms", "5000",
  ], { networkPolicy: TEST_NETWORK_POLICY });
  const draft = JSON.parse(await readFile(draftPath, "utf8"));
  assert.equal(draft.acquisitionOutcomes[0].status, "acquired");
  assert.equal(draft.entries[0].artifacts[0].observedEntries, null);
  assert.match(draft.entries[0].artifacts[0].observationError, /cargo build for inspect exited/);
  const artifact = selection.entries[0].artifacts[0];
  const journal = path.join(corpusRoot, ...artifact.path.split("/")) + ".mdictlib-lock.json";
  assert.equal(existsSync(journal), true);
  const requestsAfterFirst = fixture.totalRequests();

  const staleDraftPath = path.join(root, "stale-draft.json");
  await writeFile(staleDraftPath, "stale bootstrap evidence\n");
  const noOpCargo = path.join(root, process.platform === "win32" ? "noop-cargo.cmd" : "noop-cargo");
  await writeFile(noOpCargo, process.platform === "win32" ? "@exit /b 0\r\n" : "#!/bin/sh\nexit 0\n");
  if (process.platform !== "win32") await chmod(noOpCargo, 0o700);
  const previousTarget = process.env.CARGO_TARGET_DIR;
  process.env.CARGO_TARGET_DIR = path.join(root, "missing-target");
  try {
    await assert.rejects(
      lockCorpusMain([
        "--selection", selectionPath,
        "--inventory", inventoryPath,
        "--output", staleDraftPath,
        "--root", corpusRoot,
        "--cargo", noOpCargo,
      ], { networkPolicy: TEST_NETWORK_POLICY }),
      /ENOENT|no such file|failed to open/i,
    );
  } finally {
    if (previousTarget === undefined) delete process.env.CARGO_TARGET_DIR;
    else process.env.CARGO_TARGET_DIR = previousTarget;
  }
  assert.equal(existsSync(staleDraftPath), false);

  const legacyJournal = JSON.parse(await readFile(journal, "utf8"));
  delete legacyJournal.inventorySha256;
  delete legacyJournal.selectionSha256;
  delete legacyJournal.sourcePath;
  await writeFile(journal, stableJson(legacyJournal));

  const secondDraftPath = path.join(root, "draft-second.json");
  await lockCorpusMain([
    "--selection", selectionPath,
    "--inventory", inventoryPath,
    "--output", secondDraftPath,
    "--root", corpusRoot,
    "--skip-observe",
  ], { networkPolicy: TEST_NETWORK_POLICY });
  assert.equal(fixture.totalRequests(), requestsAfterFirst);
  assert.equal(existsSync(journal), true);
  const secondDraft = JSON.parse(await readFile(secondDraftPath, "utf8"));
  assert.match(secondDraft.entries[0].artifacts[0].observationError, /skipped/);
  const upgradedJournal = JSON.parse(await readFile(journal, "utf8"));
  assert.equal(upgradedJournal.sourcePath, artifact.sourcePath);
  assert.equal(upgradedJournal.inventorySha256, selection.source.inventorySha256);
  assert.equal(upgradedJournal.selectionSha256, secondDraft.selectionBinding.selectionSha256);

  const tamperedJournal = { ...upgradedJournal, bytes: upgradedJournal.bytes - 1 };
  await writeFile(journal, stableJson(tamperedJournal));
  const tamperedDraftPath = path.join(root, "draft-tampered-journal.json");
  const tampered = await lockCorpusMain([
    "--selection", selectionPath,
    "--inventory", inventoryPath,
    "--output", tamperedDraftPath,
    "--root", corpusRoot,
    "--skip-observe",
    "--retries", "0",
  ], { networkPolicy: TEST_NETWORK_POLICY });
  assert.equal(tampered.acquired, 0);
  assert.equal(tampered.acquisitionErrors, 1);
  assert.match(
    JSON.parse(await readFile(tamperedDraftPath, "utf8")).acquisitionOutcomes[0].error,
    /does not match the selection/,
  );
});

test("bootstrap continues after an acquisition error and promotion retains the complete outcome", async (t) => {
  const root = await temporaryRoot("acquisition-outcome");
  const fixture = await serverFixture();
  t.after(async () => {
    await fixture.close();
    await rm(root, { recursive: true, force: true });
  });
  const inventory = inventoryFixture(fixture.base, ["file", "unavailable"]);
  const { inventoryText, selection } = reviewedSelection(inventory);
  const inventoryPath = path.join(root, "inventory.json");
  const selectionPath = path.join(root, "selection.json");
  const draftPath = path.join(root, "draft.json");
  await writeFile(inventoryPath, inventoryText);
  await writeFile(selectionPath, stableJson(selection));
  const summary = await lockCorpusMain([
    "--selection", selectionPath,
    "--inventory", inventoryPath,
    "--output", draftPath,
    "--root", path.join(root, "bytes"),
    "--skip-observe",
    "--retries", "0",
    "--timeout-ms", "5000",
  ], { networkPolicy: TEST_NETWORK_POLICY });
  assert.equal(summary.acquired, 1);
  assert.equal(summary.acquisitionErrors, 1);
  const draft = JSON.parse(await readFile(draftPath, "utf8"));
  assert.deepEqual(
    draft.acquisitionOutcomes.map(({ status }) => status),
    ["acquired", "acquisition-error"],
  );
  assert.equal(draft.entries.length, 1);
  assert.match(draft.acquisitionOutcomes[1].error, /HTTP 404/);
  const promoted = promoteDraft(draft, TEST_NETWORK_POLICY);
  assert.deepEqual(
    promoted.outcomes.results.map(({ status }) => status),
    ["excluded", "acquisition-error"],
  );
  assert.equal(promoted.outcomes.results[1].url, `${fixture.base}/unavailable`);
  assert.equal(promoted.lock.entries.length, 0);
  validatePromotionPair(promoted.lock, promoted.outcomes, TEST_NETWORK_POLICY);
  const relabeledFailure = structuredClone(promoted.outcomes);
  relabeledFailure.results[1].status = "excluded";
  assert.throws(
    () => validatePromotionPair(promoted.lock, relabeledFailure, TEST_NETWORK_POLICY),
    /acquisition must be downloaded or reused|bytes/,
  );
});

test("symlinked corpus parents are rejected for download, bootstrap reuse, sync, and verification", async (t) => {
  const workspace = await temporaryRoot("symlink-parent");
  const fixture = await serverFixture();
  t.after(async () => {
    await fixture.close();
    await rm(workspace, { recursive: true, force: true });
  });
  const corpusRoot = path.join(workspace, "corpus");
  const outside = path.join(workspace, "outside");
  await mkdir(corpusRoot);
  await mkdir(outside);
  await symlink(outside, path.join(corpusRoot, "linked"), "dir");
  const requestsBefore = fixture.totalRequests();
  await assert.rejects(
    downloadAtomic({
      url: `${fixture.base}/file`,
      destination: path.join(corpusRoot, "linked", "escaped.mdx"),
      root: corpusRoot,
      maxBytes: 1024,
      timeoutMs: 1_000,
      networkPolicy: TEST_NETWORK_POLICY,
    }),
    /symlink/,
  );
  assert.equal(fixture.totalRequests(), requestsBefore);

  await writeFile(path.join(outside, "escaped.mdx"), PAYLOAD);
  const escapedArtifact = lockedArtifact(fixture.base, "linked/escaped.mdx");
  await assert.rejects(verifyArtifact(corpusRoot, escapedArtifact), /symlink/);
  const escapedLockPath = path.join(workspace, "escaped.lock.json");
  await writeFile(escapedLockPath, stableJson(lockFor(fixture.base, [escapedArtifact])));
  await assert.rejects(
    syncCorpus({
      catalogPath: escapedLockPath,
      root: corpusRoot,
      concurrency: 1,
      retries: 0,
      timeoutMs: 1_000,
      networkPolicy: TEST_NETWORK_POLICY,
    }),
    /symlink/,
  );

  const inventory = inventoryFixture(fixture.base, ["file"]);
  const { inventoryText, selection } = reviewedSelection(inventory);
  const inventoryPath = path.join(workspace, "inventory.json");
  const selectionPath = path.join(workspace, "selection.json");
  await writeFile(inventoryPath, inventoryText);
  await writeFile(selectionPath, stableJson(selection));
  const selectedArtifact = selection.entries[0].artifacts[0];
  const selectedParts = selectedArtifact.path.split("/");
  const bootstrapRoot = path.join(workspace, "bootstrap");
  await mkdir(bootstrapRoot);
  await symlink(outside, path.join(bootstrapRoot, selectedParts[0]), "dir");
  const outsideDestination = path.join(outside, ...selectedParts.slice(1));
  await mkdir(path.dirname(outsideDestination), { recursive: true });
  await writeFile(outsideDestination, PAYLOAD);
  await writeFile(`${outsideDestination}.mdictlib-lock.json`, stableJson({
    advertisedBytes: PAYLOAD.length,
    bytes: PAYLOAD.length,
    kind: selectedArtifact.kind,
    path: selectedArtifact.path,
    resolvedUrl: selectedArtifact.url,
    schemaVersion: 1,
    sha256: SHA256,
    url: selectedArtifact.url,
  }));
  const symlinkDraftPath = path.join(workspace, "symlink-draft.json");
  const bootstrap = await lockCorpusMain([
    "--selection", selectionPath,
    "--inventory", inventoryPath,
    "--output", symlinkDraftPath,
    "--root", bootstrapRoot,
    "--skip-observe",
    "--retries", "0",
  ], { networkPolicy: TEST_NETWORK_POLICY });
  assert.equal(bootstrap.acquired, 0);
  assert.equal(bootstrap.acquisitionErrors, 1);
  assert.equal(fixture.totalRequests(), requestsBefore);
  const serializedOutcome = await readFile(symlinkDraftPath, "utf8");
  assert.equal(serializedOutcome.includes(workspace), false);
  assert.match(serializedOutcome, /<corpus-root>|<corpus-artifact>/);
});

test("prospective output aliases and promote output/outcomes aliases are rejected", async (t) => {
  const root = await temporaryRoot("aliases");
  t.after(() => rm(root, { recursive: true, force: true }));
  const real = path.join(root, "real");
  await mkdir(real);
  await symlink(real, path.join(root, "alias-a"), "dir");
  await symlink(real, path.join(root, "alias-b"), "dir");
  await assert.rejects(
    assertDistinctPaths({
      first: path.join(root, "alias-a", "out.json"),
      second: path.join(root, "alias-b", "out.json"),
    }),
    /filesystem links/,
  );
  const input = path.join(root, "input.json");
  const output = path.join(root, "same.json");
  await writeFile(input, "{}\n");
  await assert.rejects(
    promoteLockMain([
      "--input", input,
      "--output", output,
      "--outcomes", output,
      "--accept-self-observed",
    ]),
    /outcomes must not alias output|output must not alias outcomes/,
  );
});

test("sync is deterministic, reuses verified files, and verify-only checks the exact manifest", async (t) => {
  const root = await temporaryRoot("sync");
  const fixture = await serverFixture();
  t.after(async () => {
    await fixture.close();
    await rm(root, { recursive: true, force: true });
  });
  const lock = lockFor(fixture.base, [
    lockedArtifact(fixture.base, "zeta/z.mdx", "/redirect"),
    lockedArtifact(fixture.base, "alpha/a.mdx"),
  ]);
  const catalogPath = path.join(root, "catalog.lock.json");
  await writeFile(catalogPath, stableJson(lock));
  const first = await syncCorpus({
    catalogPath,
    root,
    concurrency: 2,
    retries: 0,
    timeoutMs: 5_000,
    networkPolicy: TEST_NETWORK_POLICY,
  });
  assert.equal(first.downloaded, 2);
  const expected = manifestText(validateLock(lock, TEST_NETWORK_POLICY).entries.flatMap((entry) =>
    entry.artifacts.map((artifact) => ({ entry, artifact })),
  ));
  assert.equal(await readFile(path.join(root, "mdictlib-corpus.tsv"), "utf8"), expected);
  const second = await syncCorpus({
    catalogPath,
    root,
    concurrency: 2,
    retries: 0,
    timeoutMs: 5_000,
    networkPolicy: TEST_NETWORK_POLICY,
  });
  assert.equal(second.reused, 2);
  for (const entry of lock.entries) {
    for (const artifact of entry.artifacts) await verifyArtifact(root, artifact);
  }
});
