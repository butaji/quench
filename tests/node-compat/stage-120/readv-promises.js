const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-120-${process.pid}`;
  fs.writeFileSync(path, "abcd");
  const fd = fs.openSync(path, "r");
  const buffers = [Buffer.alloc(2), Buffer.alloc(2)];
  const result = await fs.promises.readv(fd, buffers, 0);
  assert.strictEqual(result.bytesRead, 4);
  assert.strictEqual(result.buffers, buffers);
  assert.strictEqual(Buffer.concat(result.buffers).toString(), "abcd");
  fs.closeSync(fd);
  fs.rmSync(path);
})().then(() => undefined);
