import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";

const snapshotUrl = new URL("../../corpus/mdict-org-2026-08-10.inventory.json", import.meta.url);

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function compareText(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function listingFingerprint(rows) {
  const text = [...rows]
    .sort((left, right) => compareText(left.url, right.url))
    .map(({ bytes, url }) => `${bytes}\t${url}\n`)
    .join("");
  return sha256(text);
}

test("committed mdict.org inventory retains its exact audited scope", async () => {
  const bytes = await readFile(snapshotUrl);
  const inventory = JSON.parse(bytes.toString("utf8"));

  assert.equal(sha256(bytes), "51ba61e985e42351ce7a1ee0c7713ac1f9f02284870383a12c3350ad2b5fa74d");
  assert.equal(inventory.schemaVersion, 1);
  assert.equal(inventory.root, "https://mdx.mdict.org/");
  assert.equal(inventory.snapshotAt, "2026-08-10T04:06:35.081Z");
  assert.equal(inventory.pageCount, 990);
  assert.equal(inventory.fileCount, 2_992);
  assert.equal(inventory.advertisedBytes, 144_841_177_042);
  assert.equal(inventory.files.length, inventory.fileCount);
  assert.equal(new Set(inventory.files.map(({ path }) => path)).size, inventory.fileCount);

  const mdx = inventory.files.filter(({ type }) => type === "mdx");
  assert.equal(mdx.length, 1_254);
  assert.equal(mdx.reduce((sum, { bytes: length }) => sum + length, 0), 40_084_630_153);
  assert.equal(
    listingFingerprint(mdx),
    "cfa8cdc0e3b1579280398a295e45b7b56fb7c5ee856aa138492cbc72e6eac77d",
  );

  const mdd = inventory.files.filter(({ type }) => type === "mdd");
  assert.equal(mdd.length, 335);
  assert.equal(mdd.reduce((sum, { bytes: length }) => sum + length, 0), 47_594_522_494);
  assert.equal(
    listingFingerprint(mdd),
    "5bd6e1a9106b128b34770a35232c2a289c47c39c628d68bcdb42d00ec9b3d823",
  );
});
