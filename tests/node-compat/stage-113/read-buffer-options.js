const fs = require("fs");
const assert = require("assert");

const path = `/tmp/quench-node-stage-113-${process.pid}`;
fs.writeFileSync(path, "x");
const fd = fs.openSync(path, "r");
const buffer = Buffer.alloc(1);
fs.read(fd, { buffer, offset: null }, (error, count, result) => {
  assert.ifError(error);
  assert.strictEqual(count, 1);
  assert.strictEqual(result, buffer);
  assert.strictEqual(result[0], 120);
  fs.closeSync(fd);
  fs.rmSync(path);
});
