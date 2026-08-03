const fs = require("fs");
const assert = require("assert");
const path = `/tmp/quench-node-stage-148-${process.pid}`;
fs.writeFileSync(path, "x");
const expected = fs.realpathSync(path);
assert.strictEqual(fs.realpathSync(path, "utf8"), expected);
assert.strictEqual(fs.realpathSync(path, "buffer").toString(), expected);
fs.rmSync(path);
