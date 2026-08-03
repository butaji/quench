const fs = require("fs");
const assert = require("assert");
const { text, pull } = require("stream/iter");
const { compressGzip, decompressGzip } = require("zlib/iter");

(async () => {
  const path = `/tmp/quench-node-stage-174-${process.pid}`;
  fs.writeFileSync(path, "bbbccc");
  const handle = await fs.promises.open(path, "r");
  const compressed = handle.pull(compressGzip(), { start: 0, limit: 6 });
  assert.strictEqual(await text(pull(compressed, decompressGzip())), "bbbccc");
  await handle.close();
  fs.rmSync(path);
})();
