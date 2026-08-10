#!/usr/bin/env node
// Bounded, opt-in acquisition of the v1-paired MDD candidates.
//
// Scope is fixed by the tracked inventory, not by an argument: the candidates
// are exactly the MDD rows whose directory and stem match a v1 MDX row in the
// tracked acquisition ledger. Everything lands in the ignored `.corpus/` cache
// and is never committed.
//
// Policy, matching the existing corpus tooling:
//   - credential- and query-free HTTPS only, single reviewed origin
//   - redirects only within that origin
//   - per-file and aggregate byte ceilings
//   - inactivity timeout and absolute per-attempt deadline
//   - every selected row retains an outcome; nothing is silently dropped
//
// Usage:
//   node scripts/corpus/acquire-v1-mdd.mjs --confirm [--max-file-bytes N]
//                                          [--max-total-bytes N] [--out <path>]

import { createHash } from "node:crypto";
import { createWriteStream } from "node:fs";
import { mkdir, rm, stat, writeFile } from "node:fs/promises";
import { get } from "node:https";
import path from "node:path";
import { pipeline } from "node:stream/promises";
import { readFileSync } from "node:fs";

const LEDGER = "corpus/mdict-org-2026-08-10.acquisition-outcomes.json";
const LEDGER_SHA256 = "f19ced0c1b844277689cc723d15a762ea81678d3b4a19ddf5e28fe1af568ea65";
const INVENTORY = "corpus/mdict-org-2026-08-10.inventory.json";
const INVENTORY_SHA256 = "51ba61e985e42351ce7a1ee0c7713ac1f9f02284870383a12c3350ad2b5fa74d";
const REVIEWED_ORIGIN = "https://mdx.mdict.org";
const DESTINATION = ".corpus/mdict-org/mdd";

const DEFAULT_MAX_FILE_BYTES = 64 * 1024 * 1024;
const DEFAULT_MAX_TOTAL_BYTES = 128 * 1024 * 1024;
const INACTIVITY_MS = 60_000;
const DEADLINE_MS = 30 * 60_000;

function fail(message) {
  console.error(`acquire-v1-mdd: ${message}`);
  process.exit(1);
}

function parseArguments(argv) {
  const options = {
    confirm: false,
    maxFileBytes: DEFAULT_MAX_FILE_BYTES,
    maxTotalBytes: DEFAULT_MAX_TOTAL_BYTES,
    out: null,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    switch (flag) {
      case "--confirm": options.confirm = true; break;
      case "--max-file-bytes": options.maxFileBytes = Number(argv[++index]); break;
      case "--max-total-bytes": options.maxTotalBytes = Number(argv[++index]); break;
      case "--out": options.out = argv[++index]; break;
      default: fail(`unknown argument ${flag}`);
    }
  }
  return options;
}

function sha256File(filePath) {
  return createHash("sha256").update(readFileSync(filePath)).digest("hex");
}

const stem = (value) =>
  decodeURIComponent(value.split("/").pop()).replace(/\.[Mm][Dd][XxDd]$/, "");
const directory = (value) => value.slice(0, value.lastIndexOf("/"));

/// Selects the MDD rows that pair with a version 1 MDX row by directory + stem.
function selectCandidates() {
  const ledger = JSON.parse(readFileSync(LEDGER, "utf8"));
  const inventory = JSON.parse(readFileSync(INVENTORY, "utf8"));
  const v1 = ledger.results.filter(
    (row) => row.status === "excluded" && /major version/i.test(String(row.observationError ?? "")),
  );
  const keys = new Set(v1.map((row) => `${directory(row.sourcePath)}/${stem(row.sourcePath)}`));
  return inventory.files
    .filter((file) => String(file.type).toLowerCase() === "mdd")
    .filter((file) => keys.has(`${directory(file.path)}/${stem(file.path)}`))
    .sort((left, right) => left.bytes - right.bytes);
}

/// Downloads one URL under the inactivity timeout and absolute deadline.
function download(url, destination, maxBytes) {
  return new Promise((resolve, reject) => {
    const parsed = new URL(url);
    if (parsed.origin !== REVIEWED_ORIGIN) {
      reject(new Error(`origin ${parsed.origin} is outside the reviewed origin`));
      return;
    }
    if (parsed.search || parsed.username || parsed.password) {
      reject(new Error("url carries a query or credentials"));
      return;
    }

    const deadline = setTimeout(() => request.destroy(new Error("absolute deadline exceeded")), DEADLINE_MS);
    const request = get(url, { timeout: INACTIVITY_MS }, (response) => {
      if (response.statusCode >= 300 && response.statusCode < 400 && response.headers.location) {
        const next = new URL(response.headers.location, url);
        response.resume();
        clearTimeout(deadline);
        if (next.origin !== REVIEWED_ORIGIN) {
          reject(new Error(`redirect to ${next.origin} leaves the reviewed origin`));
          return;
        }
        download(next.href, destination, maxBytes).then(resolve, reject);
        return;
      }
      if (response.statusCode !== 200) {
        response.resume();
        clearTimeout(deadline);
        reject(new Error(`HTTP ${response.statusCode}`));
        return;
      }

      let received = 0;
      const hash = createHash("sha256");
      response.on("data", (chunk) => {
        received += chunk.length;
        hash.update(chunk);
        if (received > maxBytes) {
          request.destroy(new Error(`exceeded per-file ceiling of ${maxBytes} bytes`));
        }
      });
      pipeline(response, createWriteStream(destination))
        .then(() => {
          clearTimeout(deadline);
          resolve({ bytes: received, sha256: hash.digest("hex") });
        })
        .catch((error) => {
          clearTimeout(deadline);
          reject(error);
        });
    });
    request.on("timeout", () => request.destroy(new Error("inactivity timeout")));
    request.on("error", (error) => {
      clearTimeout(deadline);
      reject(error);
    });
  });
}

async function main() {
  const options = parseArguments(process.argv.slice(2));

  if (sha256File(LEDGER) !== LEDGER_SHA256) fail("ledger digest mismatch");
  if (sha256File(INVENTORY) !== INVENTORY_SHA256) fail("inventory digest mismatch");

  const candidates = selectCandidates();
  const advertised = candidates.reduce((total, file) => total + file.bytes, 0);

  console.error(`candidates: ${candidates.length}`);
  console.error(`advertised bytes: ${advertised}`);
  console.error(`per-file ceiling: ${options.maxFileBytes}`);
  console.error(`aggregate ceiling: ${options.maxTotalBytes}`);
  console.error(`destination: ${DESTINATION}`);
  if (!options.confirm) {
    console.error("\nDry run. Re-run with --confirm to acquire.");
    for (const file of candidates) console.error(`  ${file.bytes}\t${file.path}`);
    return;
  }
  if (advertised > options.maxTotalBytes) {
    fail(`advertised ${advertised} bytes exceeds the aggregate ceiling ${options.maxTotalBytes}`);
  }

  await mkdir(DESTINATION, { recursive: true });
  const results = [];
  let acquired = 0;
  for (const file of candidates) {
    // Collision-safe local name: the source path's digest, not its basename.
    const localName = `${createHash("sha256").update(file.path).digest("hex").slice(0, 32)}.mdd`;
    const localPath = path.join(DESTINATION, localName);
    const row = { sourcePath: file.path, url: file.url, advertisedBytes: file.bytes, localPath: path.join("mdict-org/mdd", localName) };
    if (acquired + file.bytes > options.maxTotalBytes) {
      row.status = "skipped";
      row.error = "aggregate ceiling would be exceeded";
      results.push(row);
      continue;
    }
    try {
      const { bytes, sha256 } = await download(file.url, localPath, options.maxFileBytes);
      acquired += bytes;
      row.status = "acquired";
      row.bytes = bytes;
      row.sha256 = sha256;
      console.error(`  acquired ${bytes} ${localName}`);
    } catch (error) {
      await rm(localPath, { force: true });
      row.status = "failed";
      row.error = String(error.message).slice(0, 300);
      console.error(`  FAILED ${file.path}: ${row.error}`);
    }
    results.push(row);
  }

  const report = {
    protocol: "mdictlib-v1-mdd-acquisition-v1",
    generatedAt: new Date().toISOString(),
    reviewedOrigin: REVIEWED_ORIGIN,
    ledger: { path: LEDGER, sha256: LEDGER_SHA256 },
    inventory: { path: INVENTORY, sha256: INVENTORY_SHA256 },
    ceilings: { perFile: options.maxFileBytes, aggregate: options.maxTotalBytes },
    candidates: candidates.length,
    advertisedBytes: advertised,
    acquiredBytes: acquired,
    results,
  };
  console.log(JSON.stringify(report, null, 1));
  if (options.out) {
    await writeFile(options.out, `${JSON.stringify(report, null, 1)}\n`);
    console.error(`wrote ${options.out}`);
  }
  const acquiredCount = results.filter((row) => row.status === "acquired").length;
  console.error(`\nacquired ${acquiredCount}/${candidates.length}, ${acquired} bytes`);
}

await main();
