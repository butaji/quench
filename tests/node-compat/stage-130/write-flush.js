const fs = require("fs");
const assert = require("assert");
const path = `/tmp/quench-node-stage-130-${process.pid}`;
for (const value of ["true", 0, [], {}]) {
  assert.throws(() => fs.writeFileSync(path, "x", { flush: value }), {
    code: "ERR_INVALID_ARG_TYPE",
  });
}
fs.writeFileSync(path, "flushed", { flush: true });
assert.strictEqual(fs.readFileSync(path, "utf8"), "flushed");
fs.rmSync(path);
