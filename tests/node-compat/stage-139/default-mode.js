const fs = require("fs");
const assert = require("assert");
const path = `/tmp/quench-node-stage-139-${process.pid}`;
fs.writeFileSync(path, "mode");
assert.strictEqual(fs.statSync(path).mode & 0o777, 0o666 & ~process.umask());
fs.rmSync(path);
