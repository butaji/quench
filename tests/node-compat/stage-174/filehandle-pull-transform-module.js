const fs = require("fs");
const { text, pull } = require("stream/iter");
const { compressGzip, decompressGzip } = require("zlib/iter");

(async () => {
  const path = `/tmp/quench-node-stage-174-${process.pid}`;
  fs.writeFileSync(path, "bbbccc");
  const handle = await fs.promises.open(path, "r");
  const compressed = handle.pull(compressGzip(), { start: 0, limit: 6 });
  if ((await text(pull(compressed, decompressGzip()))) !== "bbbccc")
    throw new Error("pull transform module mismatch");
  await handle.close();
})();
