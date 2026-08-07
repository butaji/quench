const assert = require("assert");
const childProcess = require("child_process");

const child = childProcess.fork("child.js", ["child"]);
assert.throws(() => child.send("msg", null, null), {
  code: "ERR_INVALID_ARG_TYPE",
});
child.kill();

console.log("child process fork send validation passed");
