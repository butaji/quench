const fs = require("fs");
const assert = require("assert");
const path = `/tmp/quench-node-stage-114-${process.pid}`;
fs.writeFileSync(path, "x");
const fd = fs.openSync(path, "r");
assert.throws(() => fs.readSync(fd, Buffer.alloc(1), -1, 1, 0), {
  code: "ERR_OUT_OF_RANGE",
});
assert.throws(() => fs.read(fd, Buffer.alloc(1), 0, 1, "bad", () => {}), {
  code: "ERR_INVALID_ARG_TYPE",
});
fs.closeSync(fd);
fs.rmSync(path);
