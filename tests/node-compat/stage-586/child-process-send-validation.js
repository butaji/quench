const assert = require("assert");

assert.throws(() => process.send("msg", null, null), {
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => process.send("msg", "handle", undefined), {
  code: "ERR_INVALID_HANDLE_TYPE",
});

console.log("child process send validation passed");
