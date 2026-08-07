const assert = require("node:assert");
const childProcess = require("node:child_process");

assert.throws(
  () => childProcess.execFile(process.execPath, [], { signal: "invalid" }),
  { code: "ERR_INVALID_ARG_TYPE" },
);

console.log("execFile signal validation passed");
