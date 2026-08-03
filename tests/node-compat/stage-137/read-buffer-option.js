const fs = require("fs");
const assert = require("assert");
const path = `/tmp/quench-node-stage-137-${process.pid}`;
fs.writeFileSync(path, "hello");
const buffer = Buffer.alloc(8, 0x78);
const result = fs.readFileSync(path, { buffer });
assert.strictEqual(result.toString(), "hello");
assert.strictEqual(buffer[5], 0x78);
fs.rmSync(path);
