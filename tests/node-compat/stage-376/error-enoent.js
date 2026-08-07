const assert = require("assert");
const fs = require("fs");
assert.throws(
  () => fs.readFileSync(`/tmp/quench-node-no-such-${process.pid}`),
  { code: "ENOENT", syscall: "open" },
);
