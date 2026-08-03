const fs = require("fs");
const assert = require("assert");
const path = `/tmp/quench-node-stage-147-${process.pid}`;
fs.writeFileSync(path, "x");
assert.strictEqual(fs.realpathSync.native(path), fs.realpathSync(path));
fs.rmSync(path);
