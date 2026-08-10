#!/usr/bin/env node

import { readFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";
import { auditCorpus, buildAuditRunner, prepareAuditRun } from "./audit-corpus.mjs";
import {
  MANIFEST_NAME,
  approvedArtifacts,
  fail,
  manifestText,
  parseOptions,
  positiveOption,
  readJson,
  validateLock,
  verifyArtifact,
} from "./lib.mjs";
import { recordLogicalBaselines } from "./record-logical-baselines.mjs";

function run(command, args, env) {
  const result = spawnSync(command, args, { env, stdio: "inherit" });
  if (result.error) fail(`failed to run ${command}: ${result.error.message}`);
  if (result.status !== 0) fail(`${command} ${args.join(" ")} exited with status ${result.status}`);
}

export async function verifyCorpus({ catalogPath, root }) {
  const lock = validateLock(await readJson(catalogPath));
  const rows = approvedArtifacts(lock);
  if (rows.length === 0) fail("reviewed lock has no approved local-testing dictionary artifacts");
  for (const { artifact } of rows) await verifyArtifact(root, artifact);
  const manifestPath = await verifyManifest({ catalogPath, root, rows });
  return { lock, rows, manifestPath };
}

async function verifyManifest({ catalogPath, root, rows }) {
  const manifestPath = path.join(root, MANIFEST_NAME);
  const expectedManifest = manifestText(rows);
  let actualManifest;
  try {
    actualManifest = await readFile(manifestPath, "utf8");
  } catch (error) {
    fail(`failed to read ${manifestPath}: ${error instanceof Error ? error.message : String(error)}`);
  }
  if (actualManifest !== expectedManifest) {
    fail(`${manifestPath} is not the exact deterministic manifest for ${catalogPath}; run sync.mjs`);
  }
  return manifestPath;
}

export async function main(argv = process.argv.slice(2)) {
  const options = parseOptions(argv, {
    "--catalog": "string",
    "--root": "string",
    "--verify-only": "boolean",
    "--mode": "string",
    "--cargo": "string",
    "--audit-output": "string",
    "--outcomes-output": "string",
    "--audit-concurrency": "string",
    "--artifact-timeout-ms": "string",
  });
  const catalogPath = options.catalog ?? path.join("corpus", "catalog.lock.json");
  const root = path.resolve(options.root ?? ".corpus");
  const mode = options["verify-only"] ? "verify" : (options.mode ?? "full");
  if (!["verify", "quick", "full"].includes(mode)) fail("--mode must be verify, quick, or full");
  if (options["audit-output"] && mode !== "full") fail("--audit-output requires --mode full");
  if (options["outcomes-output"] && mode !== "full") fail("--outcomes-output requires --mode full");
  if ((options["audit-concurrency"] || options["artifact-timeout-ms"]) && mode !== "full") {
    fail("--audit-concurrency and --artifact-timeout-ms require --mode full");
  }
  const env = { ...process.env, MDICT_CORPUS_DIR: root };
  const cargo = options.cargo ?? "cargo";
  let exhaustive = null;
  if (mode === "full") {
    const auditOutput = options["audit-output"] ? path.resolve(options["audit-output"]) : null;
    const outcomesOutput = path.resolve(
      options["outcomes-output"] ?? path.join(root, "mdictlib-corpus-audit.outcomes.json"),
    );
    const concurrency = positiveOption(options["audit-concurrency"], 2, "--audit-concurrency");
    const timeoutMs = positiveOption(
      options["artifact-timeout-ms"],
      3_600_000,
      "--artifact-timeout-ms",
    );
    const prepared = await prepareAuditRun({
      catalogPath,
      root,
      outcomesPath: outcomesOutput,
      auditOutputPath: auditOutput,
    });
    process.stdout.write("Building the isolated one-artifact corpus audit runner.\n");
    const runner = await buildAuditRunner({ cargo, env });
    exhaustive = await auditCorpus({
      catalogPath,
      root,
      runner,
      outcomesPath: outcomesOutput,
      auditOutputPath: auditOutput,
      prepared,
      concurrency,
      timeoutMs,
      onProgress: ({ completed, total, result: artifactResult }) => {
        if (artifactResult.status === "failed" || completed === total || completed % 25 === 0) {
          process.stdout.write(
            `Exhaustive artifact audits: ${completed}/${total} complete; latest ${artifactResult.status}.\n`,
          );
        }
      },
    });
    process.stdout.write(`Exact-set exhaustive outcomes: ${outcomesOutput}\n`);
    if (!exhaustive.outcomes.completeSuccess) {
      fail(
        `exhaustive audit failed for ${exhaustive.outcomes.summary.failed} of ${exhaustive.outcomes.denominator.artifactCount} locked artifacts; inspect ${outcomesOutput}`,
      );
    }
    recordLogicalBaselines(
      exhaustive.lock,
      exhaustive.auditText,
      exhaustive.outcomes,
      {
        catalogIdentity: exhaustive.catalogIdentity,
        outcomesIdentity: exhaustive.outcomesIdentity,
      },
    );
    if (auditOutput !== null) process.stdout.write(`Exact exhaustive audit TSV: ${auditOutput}\n`);
  }

  const result =
    exhaustive === null
      ? await verifyCorpus({ catalogPath, root })
      : {
          lock: exhaustive.lock,
          rows: exhaustive.rows,
          manifestPath: await verifyManifest({ catalogPath, root, rows: exhaustive.rows }),
        };
  const selfObserved = result.rows.filter(
    ({ artifact }) => artifact.entryCountBasis === "mdictlib-self-observed",
  );
  const missingLogical = result.rows.filter(
    ({ artifact }) => artifact.keySha256 === null || artifact.payloadSha256 === null,
  );
  process.stdout.write(
    `Integrity verified for ${result.rows.length} locked artifacts.\n` +
      "Scope: locked-byte identity and regression snapshots; this does not independently prove parser correctness or redistribution rights.\n",
  );
  if (selfObserved.length > 0) {
    process.stdout.write(
      `Notice: ${selfObserved.length} entry-count baseline(s) were originally observed by mdictlib itself, not an independent implementation.\n`,
    );
  }
  if (missingLogical.length > 0) {
    process.stdout.write(
      `Notice: ${missingLogical.length} artifact(s) lack one or both reviewed logical digests; exhaustive output is snapshot evidence until those values are locked.\n`,
    );
  }
  if (mode === "verify") return;

  run(cargo, ["test", "--locked", "--all-features", "--test", "local_corpus", "--", "--ignored", "--nocapture"], env);
  run(cargo, ["test", "--locked", "--all-features", "--test", "local_sample", "--", "--ignored", "--nocapture"], env);
  process.stdout.write(
    `${mode === "full" ? "Full" : "Quick"} parser validation passed. ` +
      "Treat newly printed logical hashes as self-recorded candidates until reviewed and added to the lock.\n",
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`validate-corpus: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
