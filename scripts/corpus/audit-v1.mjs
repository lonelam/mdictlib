#!/usr/bin/env node
// Re-derives the MDict version 1 corpus evidence from tracked inputs.
//
// Two independent observations are made for every artifact:
//
//   1. A from-scratch geometry probe in this file, which walks declared
//      section geometry with no help from `mdictlib`. It is the evidence for
//      claims about wire layout, block counts, and compression strata.
//   2. The `v1_audit` example, run in an isolated timeout-bounded subprocess,
//      which is the evidence for what the parser accepts and rejects.
//
// The two are deliberately not allowed to inform each other, so agreement
// between them is meaningful. Every selected row is retained with an outcome;
// difficult files are classified, never dropped.
//
// Usage:
//   node scripts/corpus/audit-v1.mjs [--out <path>] [--concurrency N]
//                                    [--timeout-ms N] [--limit N]

import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { closeSync, openSync, readFileSync, readSync, statSync, writeFileSync } from "node:fs";
import { cpus } from "node:os";
import path from "node:path";

const LEDGER = "corpus/mdict-org-2026-08-10.acquisition-outcomes.json";
const LEDGER_SHA256 = "f19ced0c1b844277689cc723d15a762ea81678d3b4a19ddf5e28fe1af568ea65";
const CORPUS_ROOT = ".corpus";
const RUNNER = "target/release/examples/v1_audit";
const PROTOCOL = "mdictlib-v1-audit-v1";

const MAX_HEADER_BYTES = 8 * 1024 * 1024;
const MAX_KEY_INFO_BYTES = 256 * 1024 * 1024;

function fail(message) {
  console.error(`audit-v1: ${message}`);
  process.exit(1);
}

function parseArguments(argv) {
  const options = {
    out: null,
    concurrency: Math.max(1, Math.min(4, cpus().length)),
    timeoutMs: 30 * 60 * 1000,
    limit: Infinity,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    const value = argv[index + 1];
    switch (flag) {
      case "--out": options.out = value; index += 1; break;
      case "--concurrency": options.concurrency = Number(value); index += 1; break;
      case "--timeout-ms": options.timeoutMs = Number(value); index += 1; break;
      case "--limit": options.limit = Number(value); index += 1; break;
      default: fail(`unknown argument ${flag}`);
    }
  }
  if (!Number.isInteger(options.concurrency) || options.concurrency < 1) {
    fail("--concurrency must be a positive integer");
  }
  if (!Number.isFinite(options.timeoutMs) || options.timeoutMs <= 0) {
    fail("--timeout-ms must be positive");
  }
  return options;
}

function sha256File(filePath) {
  const hash = createHash("sha256");
  hash.update(readFileSync(filePath));
  return hash.digest("hex");
}

// ---------------------------------------------------------------------------
// Independent geometry probe
// ---------------------------------------------------------------------------

function readAt(fd, offset, length) {
  const buffer = Buffer.alloc(length);
  const read = readSync(fd, buffer, 0, length, offset);
  if (read !== length) throw new Error(`short read at ${offset}: wanted ${length}, got ${read}`);
  return buffer;
}

function headerFacts(fd, size) {
  const xmlLength = readAt(fd, 0, 4).readUInt32BE(0);
  if (xmlLength === 0 || xmlLength > MAX_HEADER_BYTES || 8 + xmlLength > size) {
    throw new Error(`implausible header length ${xmlLength}`);
  }
  const xml = readAt(fd, 4, xmlLength).toString("utf16le").replace(/\0+$/, "").trim();
  const attributes = new Map();
  const pattern = /([A-Za-z_][\w.:-]*)\s*=\s*"([^"]*)"/g;
  let match;
  while ((match = pattern.exec(xml))) attributes.set(match[1], match[2]);
  const tag = (/^<\s*([A-Za-z_][\w.:-]*)/.exec(xml) ?? [, null])[1];
  return { keywordOffset: 4 + xmlLength + 4, attributes, tag };
}

/// UTF-16 summary lengths count 16-bit units; other encodings count bytes.
function summaryUnitSize(encoding) {
  return /^UTF-?16/i.test(encoding ?? "") ? 2 : 1;
}

function probeGeometry(filePath) {
  const size = statSync(filePath).size;
  const fd = openSync(filePath, "r");
  try {
    const { keywordOffset, attributes, tag } = headerFacts(fd, size);
    const header = {
      tag,
      generatedByEngineVersion: attributes.get("GeneratedByEngineVersion") ?? null,
      requiredEngineVersion: attributes.get("RequiredEngineVersion") ?? null,
      encoding: attributes.get("Encoding") ?? null,
      encrypted: attributes.get("Encrypted") ?? null,
      format: attributes.get("Format") ?? null,
    };
    const unit = summaryUnitSize(header.encoding);

    const keywordHeader = readAt(fd, keywordOffset, 16);
    const keyBlockCount = keywordHeader.readUInt32BE(0);
    const entryCount = keywordHeader.readUInt32BE(4);
    const keyInfoLength = keywordHeader.readUInt32BE(8);
    const keyBlocksLength = keywordHeader.readUInt32BE(12);

    if (keyInfoLength > MAX_KEY_INFO_BYTES) throw new Error(`key info length ${keyInfoLength} implausible`);
    const keyInfoOffset = keywordOffset + 16;
    if (keyInfoOffset + keyInfoLength > size) throw new Error("keyword metadata extends past EOF");

    const keyInfo = readAt(fd, keyInfoOffset, keyInfoLength);
    let cursor = 0;
    let summedEntries = 0;
    let summedKeyComp = 0;
    const keyBlockSizes = [];
    for (let index = 0; index < keyBlockCount; index += 1) {
      if (cursor + 4 > keyInfo.length) throw new Error(`keyword row ${index} truncated at entry count`);
      summedEntries += keyInfo.readUInt32BE(cursor);
      cursor += 4;
      if (cursor + 1 > keyInfo.length) throw new Error(`keyword row ${index} truncated at first summary`);
      cursor += 1 + keyInfo.readUInt8(cursor) * unit;
      if (cursor + 1 > keyInfo.length) throw new Error(`keyword row ${index} truncated at last summary`);
      cursor += 1 + keyInfo.readUInt8(cursor) * unit;
      if (cursor + 8 > keyInfo.length) throw new Error(`keyword row ${index} truncated at block sizes`);
      const compressed = keyInfo.readUInt32BE(cursor);
      cursor += 8;
      if (compressed < 8) throw new Error(`key block ${index} is ${compressed} bytes`);
      summedKeyComp += compressed;
      keyBlockSizes.push(compressed);
    }
    if (cursor !== keyInfo.length) {
      throw new Error(`keyword metadata has ${keyInfo.length - cursor} trailing bytes`);
    }
    if (summedEntries !== entryCount) {
      throw new Error(`keyword entries summed ${summedEntries} but header declares ${entryCount}`);
    }
    if (summedKeyComp !== keyBlocksLength) {
      throw new Error(`key blocks summed ${summedKeyComp} but header declares ${keyBlocksLength}`);
    }

    const keyBlocksOffset = keyInfoOffset + keyInfoLength;
    const recordOffset = keyBlocksOffset + keyBlocksLength;
    if (recordOffset + 16 > size) throw new Error("record header extends past EOF");

    const recordHeader = readAt(fd, recordOffset, 16);
    const recordBlockCount = recordHeader.readUInt32BE(0);
    const recordEntryCount = recordHeader.readUInt32BE(4);
    const recordIndexLength = recordHeader.readUInt32BE(8);
    const recordBlocksLength = recordHeader.readUInt32BE(12);

    if (recordEntryCount !== entryCount) {
      throw new Error(`record entries ${recordEntryCount} disagree with key entries ${entryCount}`);
    }
    if (recordIndexLength !== recordBlockCount * 8) {
      throw new Error(`record index length ${recordIndexLength} is not ${recordBlockCount} * 8`);
    }
    const recordIndexOffset = recordOffset + 16;
    if (recordIndexOffset + recordIndexLength > size) throw new Error("record index extends past EOF");

    const recordIndex = readAt(fd, recordIndexOffset, recordIndexLength);
    let summedRecordComp = 0;
    let summedRecordDecomp = 0;
    for (let index = 0; index < recordBlockCount; index += 1) {
      const compressed = recordIndex.readUInt32BE(index * 8);
      if (compressed < 8) throw new Error(`record block ${index} is ${compressed} bytes`);
      summedRecordComp += compressed;
      summedRecordDecomp += recordIndex.readUInt32BE(index * 8 + 4);
    }
    if (summedRecordComp !== recordBlocksLength) {
      throw new Error(`record blocks summed ${summedRecordComp} but header declares ${recordBlocksLength}`);
    }
    const sectionEnd = recordIndexOffset + recordIndexLength + recordBlocksLength;
    if (sectionEnd !== size) {
      throw new Error(`section end ${sectionEnd} does not equal file size ${size} (delta ${size - sectionEnd})`);
    }

    // Compression tags, read without decoding any payload.
    const keyTags = new Set();
    const recordTags = new Set();
    let blockCursor = keyBlocksOffset;
    for (const compressed of keyBlockSizes) {
      keyTags.add(readAt(fd, blockCursor, 4).toString("hex"));
      blockCursor += compressed;
    }
    blockCursor = recordIndexOffset + recordIndexLength;
    for (let index = 0; index < recordBlockCount; index += 1) {
      recordTags.add(readAt(fd, blockCursor, 4).toString("hex"));
      blockCursor += recordIndex.readUInt32BE(index * 8);
    }

    return {
      conforms: true,
      header,
      wire: {
        keywordHeaderBytes: 16,
        keywordMetadataRaw: true,
        summaryUnitSize: unit,
        keyBlockCount,
        entryCount,
        keyInfoLength,
        keyBlocksLength,
        recordBlockCount,
        recordIndexLength,
        recordBlocksLength,
        totalDecodedRecordLength: summedRecordDecomp,
        recordIndexRowBytes: 8,
      },
      compressionTags: {
        key: [...keyTags].sort(),
        record: [...recordTags].sort(),
      },
    };
  } catch (error) {
    return { conforms: false, geometryError: String(error.message ?? error) };
  } finally {
    closeSync(fd);
  }
}

// ---------------------------------------------------------------------------
// Isolated parser subprocess
// ---------------------------------------------------------------------------

function runAudit(filePath, timeoutMs) {
  return new Promise((resolve) => {
    const child = spawn(RUNNER, [filePath], { stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    let settled = false;
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      child.kill("SIGKILL");
      resolve({ status: "rejected", category: "timeout", message: `exceeded ${timeoutMs} ms` });
    }, timeoutMs);

    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", (error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve({ status: "rejected", category: "runner-error", message: String(error.message) });
    });
    child.on("close", (code, signal) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      const line = stdout.trim().split("\n").pop() ?? "";
      if (code !== 0 || !line.startsWith("{")) {
        resolve({
          status: "rejected",
          category: signal ? "runner-signal" : "runner-error",
          message: (stderr.trim() || `exit ${code}${signal ? ` signal ${signal}` : ""}`).slice(0, 512),
        });
        return;
      }
      try {
        const parsed = JSON.parse(line);
        if (parsed.protocol !== PROTOCOL) {
          resolve({ status: "rejected", category: "runner-error", message: "unexpected protocol" });
          return;
        }
        resolve(parsed);
      } catch (error) {
        resolve({ status: "rejected", category: "runner-error", message: String(error.message) });
      }
    });
  });
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

async function main() {
  const options = parseArguments(process.argv.slice(2));

  const ledgerDigest = sha256File(LEDGER);
  if (ledgerDigest !== LEDGER_SHA256) {
    fail(`ledger digest mismatch: expected ${LEDGER_SHA256}, got ${ledgerDigest}`);
  }
  try {
    statSync(RUNNER);
  } catch {
    fail(`missing ${RUNNER}; build it with:\n  cargo build --locked --release --all-features --example v1_audit`);
  }
  const runnerStat = statSync(RUNNER);
  const runnerDigest = sha256File(RUNNER);

  const ledger = JSON.parse(readFileSync(LEDGER, "utf8"));
  const rows = ledger.results
    .filter((row) => row.status === "excluded" && /major version/i.test(String(row.observationError ?? "")))
    .sort((left, right) => (left.sourcePath < right.sourcePath ? -1 : 1))
    .slice(0, options.limit);
  if (rows.length === 0) fail("no version 1 rows selected");

  // Exact denominator digest, pinned by a rule recorded here rather than
  // inferred later: sorted by source path, one `<sha256>\t<bytes>\t<path>`
  // record per row.
  const denominator = createHash("sha256");
  for (const row of rows) denominator.update(`${row.sha256}\t${row.bytes}\t${row.sourcePath}\n`);
  const denominatorDigest = denominator.digest("hex");

  console.error(`auditing ${rows.length} artifacts with concurrency ${options.concurrency}`);

  const results = new Array(rows.length);
  let next = 0;
  let done = 0;
  async function worker() {
    while (true) {
      const index = next;
      next += 1;
      if (index >= rows.length) return;
      const row = rows[index];
      const filePath = path.join(CORPUS_ROOT, row.path);
      const geometry = probeGeometry(filePath);
      const audit = await runAudit(filePath, options.timeoutMs);
      results[index] = {
        localPath: row.path,
        sourcePath: row.sourcePath,
        sha256: row.sha256,
        bytes: row.bytes,
        geometry,
        parser: audit,
      };
      done += 1;
      if (done % 25 === 0) console.error(`  ${done}/${rows.length}`);
    }
  }
  await Promise.all(Array.from({ length: options.concurrency }, worker));

  const conforming = results.filter((row) => row.geometry.conforms);
  const nonConforming = results.filter((row) => !row.geometry.conforms);
  const accepted = results.filter((row) => row.parser.status === "accepted");
  const rejected = results.filter((row) => row.parser.status !== "accepted");

  const sum = (rows, pick) => rows.reduce((total, row) => total + pick(row), 0);
  const tally = (rows, pick) => {
    const counts = new Map();
    for (const row of rows) {
      const key = pick(row);
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
    return Object.fromEntries([...counts].sort((a, b) => b[1] - a[1]));
  };

  const summary = {
    artifacts: results.length,
    totalBytes: sum(results, (row) => row.bytes),
    geometry: {
      conforming: conforming.length,
      nonConforming: nonConforming.length,
      declaredEntries: sum(conforming, (row) => row.geometry.wire.entryCount),
      keyBlocks: sum(conforming, (row) => row.geometry.wire.keyBlockCount),
      recordBlocks: sum(conforming, (row) => row.geometry.wire.recordBlockCount),
      compressionStrata: tally(
        conforming,
        (row) => `key=${row.geometry.compressionTags.key.join(",")} record=${row.geometry.compressionTags.record.join(",")}`,
      ),
      failures: nonConforming.map((row) => ({
        localPath: row.localPath,
        error: row.geometry.geometryError,
      })),
    },
    parser: {
      accepted: accepted.length,
      rejected: rejected.length,
      acceptedBytes: sum(accepted, (row) => row.bytes),
      acceptedEntries: sum(accepted, (row) => row.parser.entries ?? 0),
      rejectedCategories: tally(rejected, (row) => row.parser.category ?? "unknown"),
      encodingsAccepted: tally(accepted, (row) => row.geometry.header?.encoding ?? "absent"),
      encodingsRejected: tally(rejected, (row) => row.geometry.header?.encoding ?? "absent"),
    },
    agreement: {
      geometryConformsAndParserAccepts: results.filter(
        (row) => row.geometry.conforms && row.parser.status === "accepted",
      ).length,
      geometryConformsButParserRejects: results.filter(
        (row) => row.geometry.conforms && row.parser.status !== "accepted",
      ).length,
      geometryFailsButParserAccepts: results.filter(
        (row) => !row.geometry.conforms && row.parser.status === "accepted",
      ).length,
      geometryFailsAndParserRejects: results.filter(
        (row) => !row.geometry.conforms && row.parser.status !== "accepted",
      ).length,
    },
  };

  const report = {
    protocol: "mdictlib-v1-corpus-audit-v1",
    generatedAt: new Date().toISOString(),
    ledger: { path: LEDGER, sha256: ledgerDigest },
    denominator: {
      rule: "sha256 over sorted `<sha256>\\t<bytes>\\t<sourcePath>\\n` records, sorted by sourcePath",
      artifacts: rows.length,
      sha256: denominatorDigest,
    },
    runner: { path: RUNNER, bytes: runnerStat.size, sha256: runnerDigest },
    host: { platform: process.platform, arch: process.arch, node: process.version },
    concurrency: options.concurrency,
    timeoutMs: options.timeoutMs,
    summary,
    results,
  };

  console.log(JSON.stringify(summary, null, 2));
  if (options.out) {
    writeFileSync(options.out, `${JSON.stringify(report, null, 1)}\n`);
    console.error(`wrote ${options.out}`);
  }
}

await main();
