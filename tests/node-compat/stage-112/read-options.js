const fs = require("fs");
const assert = require("assert");

const path = `/tmp/quench-node-stage-112-${process.pid}`;
fs.writeFileSync(path, "xyz\n");
const fd = fs.openSync(path, "r");
const buffer = Buffer.alloc(4);
assert.strictEqual(fs.readSync(fd, buffer, { length: 4, position: 0 }), 4);
assert.strictEqual(buffer.toString(), "xyz\n");
fs.read(fd, { length: 4, position: 0 }, (error, count, result) => {
  assert.ifError(error);
  assert.strictEqual(count, 4);
  assert.strictEqual(result.toString(), "xyz\n");
  fs.closeSync(fd);
  fs.rmSync(path);
});
