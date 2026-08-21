const assert = require("assert");
const fs = require("fs");
const path = require("path");
const { text, pull } = require("stream/iter");
const { compressGzip, decompressGzip } = require("zlib/iter");

const root = fs.mkdtempSync(path.join(process.cwd(), "pull-zlib-"));
const jobs = ["aaabbbcccddd", "bbbccc", "0123456789"].map(
  async (value, index) => {
    const filename = path.join(root, `input-${index}`);
    fs.writeFileSync(filename, value);
    const handle = await fs.promises.open(filename, "r");
    try {
      const compressed = handle.pull(compressGzip(), {
        start: 0,
        limit: value.length,
      });
      assert.strictEqual(await text(pull(compressed, decompressGzip())), value);
    } finally {
      await handle.close();
    }
  },
);

Promise.all(jobs).then(() =>
  console.log("concurrent pull zlib contracts passed")
);
