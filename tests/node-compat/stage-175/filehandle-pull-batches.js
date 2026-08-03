const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-175-${process.pid}`;
  fs.writeFileSync(path, "abcdefghij");
  const handle = await fs.promises.open(path, "r");
  const batches = [];
  for await (const batch of handle.pull({ chunkSize: 2 })) {
    assert.strictEqual(Array.isArray(batch), true);
    assert.strictEqual(batch.length, 1);
    assert.strictEqual(batch[0].byteLength <= 2, true);
    batches.push(batch[0]);
  }
  assert.strictEqual(batches.length, 5);
  await handle.close();
  fs.rmSync(path);
})();
