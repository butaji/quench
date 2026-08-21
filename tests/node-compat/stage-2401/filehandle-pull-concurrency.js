const assert = require("assert");
const fs = require("fs");
const path = require("path");
const { text, bytes } = require("stream/iter");

const root = fs.mkdtempSync(path.join(process.cwd(), "pull-concurrent-"));
const file = (name, data) => {
  const filename = path.join(root, name);
  fs.writeFileSync(filename, data);
  return filename;
};

Promise.all([
  (async () => {
    const fh = await fs.promises.open(file("basic", "hello"), "r");
    try {
      assert.strictEqual(await text(fh.pull()), "hello");
    } finally {
      await fh.close();
    }
  })(),
  (async () => {
    const fh = await fs.promises.open(file("binary", Buffer.alloc(32, 7)), "r");
    try {
      assert.strictEqual((await bytes(fh.pull())).byteLength, 32);
    } finally {
      await fh.close();
    }
  })(),
  (async () => {
    const fh = await fs.promises.open(file("range", "0123456789"), "r");
    try {
      assert.strictEqual(await text(fh.pull({ start: 3, limit: 4 })), "3456");
    } finally {
      await fh.close();
    }
  })(),
  (async () => {
    const fh = await fs.promises.open(file("chunks", "abcdefgh"), "r");
    try {
      let count = 0;
      for await (const batch of fh.pull({ chunkSize: 2 })) {
        count += batch[0].byteLength;
      }
      assert.strictEqual(count, 8);
    } finally {
      await fh.close();
    }
  })(),
  (async () => {
    const fh = await fs.promises.open(file("abort", "abcdef"), "r");
    try {
      const controller = new AbortController();
      controller.abort();
      await assert.rejects(text(fh.pull({ signal: controller.signal })), {
        name: "AbortError",
      });
    } finally {
      await fh.close();
    }
  })(),
]).then(() => console.log("concurrent pull contracts passed"));
