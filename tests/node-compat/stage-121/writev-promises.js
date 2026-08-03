const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-121-${process.pid}`;
  const fd = fs.openSync(path, "w+");
  const buffers = [Buffer.from("ab"), Buffer.from("cd")];
  const result = await fs.promises.writev(fd, buffers, 0);
  fs.closeSync(fd);
  assert.strictEqual(result.bytesWritten, 4);
  assert.strictEqual(fs.readFileSync(path, "utf8"), "abcd");
  fs.rmSync(path);
})().then(() => undefined);
