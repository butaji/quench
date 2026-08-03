const fs = require("fs");
const assert = require("assert");

const path = `/tmp/quench-node-stage-115-${process.pid}`;
fs.writeFileSync(path, "abcd");
const fd = fs.openSync(path, "r");
const buffers = [Buffer.alloc(2), Buffer.alloc(2)];
assert.strictEqual(fs.readvSync(fd, buffers, 0), 4);
assert.strictEqual(Buffer.concat(buffers).toString(), "abcd");
const callbackBuffers = [Buffer.alloc(2), Buffer.alloc(2)];
fs.readv(fd, callbackBuffers, 0, (error, bytesRead, result) => {
  assert.ifError(error);
  assert.strictEqual(bytesRead, 4);
  assert.strictEqual(result, callbackBuffers);
  assert.strictEqual(Buffer.concat(result).toString(), "abcd");
  fs.promises.readv(fd, [Buffer.alloc(4)], 0).then((value) => {
    assert.strictEqual(value.bytesRead, 4);
    assert.strictEqual(value.buffers[0].toString(), "abcd");
    fs.closeSync(fd);
    fs.rmSync(path);
  });
});
