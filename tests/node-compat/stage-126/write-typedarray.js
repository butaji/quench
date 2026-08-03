const fs = require("fs");
const assert = require("assert");
const path = `/tmp/quench-node-stage-126-${process.pid}`;
fs.writeFileSync(path, Uint8Array.from([0x61, 0x62, 0x63]));
assert.strictEqual(fs.readFileSync(path, "utf8"), "abc");
fs.rmSync(path);
