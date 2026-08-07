const fs = require("fs");
const assert = require("assert");
const path = `/tmp/quench-node-stage-119-${process.pid}`;
const fd = fs.openSync(path, "w");
assert.throws(() => fs.writevSync(fd, {}), {
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => fs.writev(fd, {}, null, () => {}), {
  code: "ERR_INVALID_ARG_TYPE",
});
fs.closeSync(fd);
fs.rmSync(path);
