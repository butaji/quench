const assert = require("assert");
const fs = require("fs");
const { textSync, bytesSync, pipeToSync } = require("stream/iter");
const { compressGzipSync, decompressGzipSync } = require("zlib/iter");

(async () => {
  const base = `/tmp/pullsync-cluster-${process.pid}`;
  fs.writeFileSync(`${base}-basic`, "hello");
  let h = await fs.promises.open(`${base}-basic`, "r");
  assert.strictEqual(textSync(h.pullSync()), "hello");
  await h.close();
  const input = Buffer.from("binary data");
  fs.writeFileSync(`${base}-bin`, input);
  h = await fs.promises.open(`${base}-bin`, "r");
  assert.deepStrictEqual(Buffer.from(bytesSync(h.pullSync())), input);
  await h.close();
  fs.writeFileSync(`${base}-src`, "compress me ".repeat(100));
  const source = await fs.promises.open(`${base}-src`, "r");
  const destination = await fs.promises.open(`${base}-gz`, "w");
  pipeToSync(source.pullSync(compressGzipSync()), destination.writer());
  await source.close();
  await destination.close();
  const compressed = await fs.promises.open(`${base}-gz`, "r");
  assert.strictEqual(
    textSync(compressed.pullSync(decompressGzipSync())),
    "compress me ".repeat(100)
  );
  await compressed.close();
  console.log("pullSync first cluster passed");
})();
