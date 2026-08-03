const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-124-${process.pid}`;
  fs.writeFileSync(path, "abcd");
  const handle = await fs.promises.open(path, "r");
  const buffer = Buffer.alloc(4);
  const result = await handle.read(buffer, 0, 4, 0);
  assert.strictEqual(result.bytesRead, 4);
  assert.strictEqual(result.buffer, buffer);
  assert.strictEqual(result.buffer.toString(), "abcd");
  await handle.close();
  fs.rmSync(path);
})().then(() => undefined);
