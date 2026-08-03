const fs = require("fs");
const assert = require("assert");

const path = `/tmp/quench-node-stage-133-${process.pid}`;
fs.writeFileSync(path, "abcdef");
const fd = fs.openSync(path, "r+");
fs.ftruncateSync(fd, 3);
assert.strictEqual(fs.statSync(path).size, 3);
fs.ftruncateSync(fd, 1);
assert.strictEqual(fs.statSync(path).size, 1);
fs.closeSync(fd);
fs.rmSync(path);
