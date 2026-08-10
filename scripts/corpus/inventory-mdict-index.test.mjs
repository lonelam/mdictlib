import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { promisify } from "node:util";
import test from "node:test";

import {
  inventoryMdictIndex,
  parseCliArgs,
  scriptPath,
} from "./inventory-mdict-index.mjs";

const execFileAsync = promisify(execFile);
const SNAPSHOT = "2026-08-10T00:00:00.000Z";

function row(href, order) {
  const escaped = href.replaceAll("&", "&amp;").replaceAll('"', "&quot;");
  return `<tr class="file"><td><a href="${escaped}">entry</a></td><td data-order="${order}">size</td></tr>`;
}

function index(...rows) {
  return `<!doctype html><html><body><table>${rows.join("")}</table><a href="?sort=name">sort</a><a href="https://example.invalid/footer">footer</a></body></html>`;
}

async function withServer(routes, callback) {
  const hits = [];
  const server = http.createServer((request, response) => {
    hits.push(request.url);
    const route = routes[request.url];
    if (!route) {
      response.writeHead(404, { "content-type": "text/plain" });
      response.end("not found");
      return;
    }
    const send = () => {
      response.writeHead(route.status ?? 200, { "content-type": "text/html; charset=utf-8" });
      response.end(route.body);
    };
    if (route.delayMs) setTimeout(send, route.delayMs);
    else send();
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  const root = `http://127.0.0.1:${address.port}/catalog/`;
  try {
    return await callback({ root, hits });
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

test("recurses directory rows, strips sort queries, and inventories every direct file", async () => {
  const routes = {
    "/catalog/": {
      body: index(
        row("./b/?sort=size&order=desc", -1),
        row("./a/?sort=name&order=asc", -1),
        row("./b/?sort=name&order=asc", -1),
        row("./Root.MDX", 12),
      ),
    },
    "/catalog/a/": {
      body: index(row("./nested/?sort=time", -1), row("./same.MDX", 5), row("./theme.CSS", 7)),
    },
    "/catalog/a/nested/": { body: index(row("./deep.mDd", 9)) },
    "/catalog/b/": { body: index(row("./same.mdx", 6), row("./README", 4)) },
  };

  await withServer(routes, async ({ root, hits }) => {
    const inventory = await inventoryMdictIndex({ root, snapshotAt: SNAPSHOT, concurrency: 3 });
    assert.deepEqual(inventory, {
      schemaVersion: 1,
      root,
      snapshotAt: SNAPSHOT,
      pageCount: 4,
      fileCount: 6,
      advertisedBytes: 43,
      files: [
        { path: "Root.MDX", type: "mdx", bytes: 12, url: `${root}Root.MDX`, parent: "" },
        { path: "a/nested/deep.mDd", type: "mdd", bytes: 9, url: `${root}a/nested/deep.mDd`, parent: "a/nested" },
        { path: "a/same.MDX", type: "mdx", bytes: 5, url: `${root}a/same.MDX`, parent: "a" },
        { path: "a/theme.CSS", type: "css", bytes: 7, url: `${root}a/theme.CSS`, parent: "a" },
        { path: "b/README", type: "other", bytes: 4, url: `${root}b/README`, parent: "b" },
        { path: "b/same.mdx", type: "mdx", bytes: 6, url: `${root}b/same.mdx`, parent: "b" },
      ],
    });
    assert.deepEqual([...hits].sort(), ["/catalog/", "/catalog/a/", "/catalog/a/nested/", "/catalog/b/"].sort());
  });
});

test("decodes safe source paths while retaining encoded download URLs", async () => {
  await withServer(
    {
      "/catalog/": { body: index(row("./%E8%AF%8D%E5%85%B8%20One.mDx", 11)) },
    },
    async ({ root }) => {
      const inventory = await inventoryMdictIndex({ root, snapshotAt: SNAPSHOT });
      assert.equal(inventory.files[0].path, "词典 One.mDx");
      assert.equal(inventory.files[0].type, "mdx");
      assert.equal(inventory.files[0].url, `${root}%E8%AF%8D%E5%85%B8%20One.mDx`);
    },
  );
});

test("rejects traversal and cross-origin rows", async (context) => {
  await context.test("plain traversal", async () => {
    await withServer(
      { "/catalog/": { body: index(row("../escape.mdx", 1)) } },
      async ({ root }) => {
        await assert.rejects(inventoryMdictIndex({ root, snapshotAt: SNAPSHOT }), /path traversal/);
      },
    );
  });

  await context.test("encoded traversal", async () => {
    await withServer(
      { "/catalog/": { body: index(row("./%2e%2e/escape.mdx", 1)) } },
      async ({ root }) => {
        await assert.rejects(inventoryMdictIndex({ root, snapshotAt: SNAPSHOT }), /path traversal/);
      },
    );
  });

  await context.test("cross origin", async () => {
    await withServer(
      { "/catalog/": { body: index(row("https://example.invalid/evil.mdx", 1)) } },
      async ({ root }) => {
        await assert.rejects(inventoryMdictIndex({ root, snapshotAt: SNAPSHOT }), /must remain on origin/);
      },
    );
  });
});

test("stable sorting makes repeated concurrent inventories deterministic", async () => {
  const routes = {
    "/catalog/": { body: index(row("./z/?sort=name", -1), row("./a/?sort=size", -1)) },
    "/catalog/a/": { body: index(row("./two.mdx", 2)), delayMs: 20 },
    "/catalog/z/": { body: index(row("./one.mdx", 1)) },
  };
  await withServer(routes, async ({ root }) => {
    const options = { root, snapshotAt: SNAPSHOT, concurrency: 2 };
    const first = await inventoryMdictIndex(options);
    const second = await inventoryMdictIndex(options);
    assert.deepEqual(first, second);
    assert.deepEqual(first.files.map((file) => file.path), ["a/two.mdx", "z/one.mdx"]);
  });
});

test("enforces page, file, page-byte, and timeout limits", async (context) => {
  await context.test("page count", async () => {
    await withServer(
      {
        "/catalog/": { body: index(row("./child/", -1)) },
        "/catalog/child/": { body: index() },
      },
      async ({ root }) => {
        await assert.rejects(
          inventoryMdictIndex({ root, snapshotAt: SNAPSHOT, maxPages: 1 }),
          /maxPages limit of 1/,
        );
      },
    );
  });

  await context.test("file count", async () => {
    await withServer(
      { "/catalog/": { body: index(row("./one.mdx", 1), row("./two.mdx", 2)) } },
      async ({ root }) => {
        await assert.rejects(
          inventoryMdictIndex({ root, snapshotAt: SNAPSHOT, maxFiles: 1 }),
          /maxFiles limit of 1/,
        );
      },
    );
  });

  await context.test("page bytes", async () => {
    await withServer(
      { "/catalog/": { body: index(row("./one.mdx", 1)) } },
      async ({ root }) => {
        await assert.rejects(
          inventoryMdictIndex({ root, snapshotAt: SNAPSHOT, maxPageBytes: 8 }),
          /max-page-bytes/,
        );
      },
    );
  });

  await context.test("aggregate in-flight page bytes", async () => {
    await withServer(
      { "/catalog/": { body: index() } },
      async ({ root }) => {
        await assert.rejects(
          inventoryMdictIndex({
            root,
            snapshotAt: SNAPSHOT,
            concurrency: 4,
            maxPageBytes: 16,
            maxInFlightPageBytes: 63,
          }),
          /maxInFlightPageBytes limit of 63/,
        );
      },
    );
  });

  await context.test("request timeout", async () => {
    await withServer(
      { "/catalog/": { body: index(), delayMs: 100 } },
      async ({ root }) => {
        await assert.rejects(
          inventoryMdictIndex({ root, snapshotAt: SNAPSHOT, timeoutMs: 20 }),
          /timed out after 20 ms/,
        );
      },
    );
  });
});

test("CLI writes the requested deterministic inventory path", async () => {
  assert.equal(
    parseCliArgs(["--output", "inventory.json", "--max-pages", "3"]).output,
    "inventory.json",
  );
  assert.equal(parseCliArgs(["--output", "-"]).output, "-");
  assert.equal(
    parseCliArgs(["--output", "-", "--max-in-flight-page-bytes", "1234"])
      .maxInFlightPageBytes,
    1234,
  );
  assert.throws(() => parseCliArgs([]), /--output is required/);

  await withServer(
    { "/catalog/": { body: index(row("./fixture.mdx", 42)) } },
    async ({ root }) => {
      const directory = await mkdtemp(path.join(os.tmpdir(), "mdictlib-inventory-test-"));
      const output = path.join(directory, "nested", "inventory.json");
      try {
        await execFileAsync(process.execPath, [
          scriptPath,
          "--root",
          root,
          "--output",
          output,
          "--snapshot-at",
          SNAPSHOT,
          "--timeout-ms",
          "1000",
        ]);
        const parsed = JSON.parse(await readFile(output, "utf8"));
        assert.equal(parsed.root, root);
        assert.equal(parsed.snapshotAt, SNAPSHOT);
        assert.deepEqual(parsed.files.map((file) => file.path), ["fixture.mdx"]);
      } finally {
        await rm(directory, { recursive: true, force: true });
      }
    },
  );
});
