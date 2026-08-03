const assert = require("assert");
const { spawn } = require("child_process");

assert.throws(() => spawn(), { code: "ERR_INVALID_ARG_TYPE" });
assert.throws(() => spawn(""), { code: "ERR_INVALID_ARG_VALUE" });
assert.throws(() => spawn("node", "bad"), { code: "ERR_INVALID_ARG_TYPE" });
assert.throws(() => spawn("node", [], 1), { code: "ERR_INVALID_ARG_TYPE" });

console.log("child process spawn validation passed");
