const fs = require("fs");
const assert = require("assert");
const path = `/tmp/quench-node-stage-117-${process.pid}`;
fs.writeFileSync(path, "stale-data");
const fd = fs.openSync(path, "w");
fs.writeSync(fd, Buffer.from("fresh"));
fs.closeSync(fd);
assert.strictEqual(fs.readFileSync(path).toString(), "fresh");
fs.rmSync(path);
