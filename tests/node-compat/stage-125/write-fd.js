const fs = require("fs");
const assert = require("assert");
const path = `/tmp/quench-node-stage-125-${process.pid}`;
const fd = fs.openSync(path, "w+");
fs.writeFileSync(fd, "via-fd");
fs.closeSync(fd);
assert.strictEqual(fs.readFileSync(path, "utf8"), "via-fd");
fs.rmSync(path);
