const fs = require("fs");
const assert = require("assert");
const path = `/tmp/quench-node-stage-138-${process.pid}`;
const value = fs.readFileSync(path, { flag: "a+", encoding: "utf8" });
assert.strictEqual(value, "");
assert.strictEqual(fs.existsSync(path), true);
fs.rmSync(path);
