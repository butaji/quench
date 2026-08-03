const fs = require("fs");
const assert = require("assert");
const path = `/tmp/quench-node-stage-127-${process.pid}`;
const bytes = new Uint8Array([0x61, 0x62, 0x63]);
fs.writeFileSync(path, new DataView(bytes.buffer));
assert.strictEqual(fs.readFileSync(path, "utf8"), "abc");
fs.rmSync(path);
