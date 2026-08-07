const fs = require("fs");
const assert = require("assert");

const path = `/tmp/quench-node-stage-116-${process.pid}`;
const fd = fs.openSync(path, "w+");
assert.strictEqual(fs.writeSync(fd, Buffer.from("abcd")), 4);
const buffers = [Buffer.alloc(2), Buffer.alloc(2)];
assert.strictEqual(fs.readvSync(fd, buffers, 0), 4);
assert.strictEqual(Buffer.concat(buffers).toString(), "abcd");
const output = `/tmp/quench-node-stage-116-output-${process.pid}`;
const outputFd = fs.openSync(output, "w");
assert.strictEqual(
  fs.writevSync(outputFd, [Buffer.from("ab"), Buffer.from("cd")], 0),
  4,
);
fs.closeSync(outputFd);
assert.strictEqual(fs.readFileSync(output, "utf8"), "abcd");
try {
  fs.closeSync(fd);
} catch (error) {
  assert.strictEqual(error.code, "EBADF");
}
fs.rmSync(path);
fs.rmSync(output);
