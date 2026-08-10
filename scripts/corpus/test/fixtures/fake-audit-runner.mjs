#!/usr/bin/env node

import { createHash } from "node:crypto";
import { writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const [kind, artifactPath, entries] = process.argv.slice(2);
const name = path.basename(artifactPath ?? "");

if (name.startsWith("hang")) {
  setInterval(() => {}, 1_000);
} else if (name.startsWith("replace")) {
  writeFileSync(fileURLToPath(import.meta.url), "// replaced while the runner was executing\n");
  const key = createHash("sha256").update(`key:${artifactPath}`).digest("hex");
  const payload = createHash("sha256").update(`payload:${artifactPath}`).digest("hex");
  process.stdout.write(`mdictlib-corpus-audit-v1\t${kind}\t${entries}\t${key}\t${payload}\n`);
} else if (name.startsWith("fail")) {
  process.stderr.write(`synthetic ${artifactPath} ${process.cwd()}\n${"x".repeat(3_000)}\0failure\n`);
  process.exitCode = 7;
} else if (name.startsWith("spam")) {
  process.stdout.write("x".repeat(70 * 1_024));
} else if (name.startsWith("malformed")) {
  process.stdout.write("not-the-protocol\n");
} else {
  const key = createHash("sha256").update(`key:${artifactPath}`).digest("hex");
  const payload = createHash("sha256").update(`payload:${artifactPath}`).digest("hex");
  process.stdout.write(`mdictlib-corpus-audit-v1\t${kind}\t${entries}\t${key}\t${payload}\n`);
}
