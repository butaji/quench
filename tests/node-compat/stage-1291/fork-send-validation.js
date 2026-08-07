const assert = require("node:assert");
const { fork } = require("node:child_process");

const child = fork("child.js", []);
assert.throws(() => child.send(), {
  code: "ERR_MISSING_ARGS",
  message: 'The "message" argument must be specified',
});
assert.throws(() => child.send(Symbol()), {
  code: "ERR_INVALID_ARG_TYPE",
  message: /Received type symbol \(Symbol\(\)\)/,
});

console.log("fork send validation passed");
