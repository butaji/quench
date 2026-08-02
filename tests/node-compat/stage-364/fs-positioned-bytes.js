const assert = require("assert");
const fs = require("fs");
const path = `/tmp/quench-node-task-364-${process.pid}`;
try {
  fs.writeFileSync(path, Buffer.from("abcdef"));
  const fd = fs.openSync(path, "r+");
  const read = Buffer.alloc(2);
  assert.strictEqual(fs.readSync(fd, read, 0, 2, 2), 2);
  assert.strictEqual(read.toString(), "cd");
  assert.strictEqual(fs.writeSync(fd, Buffer.from("XY"), 0, 2, 1), 2);
  fs.closeSync(fd);
  assert.strictEqual(fs.readFileSync(path, "utf8"), "aXYdef");
} finally {
  try {
    fs.unlinkSync(path);
  } catch (_) {}
}
