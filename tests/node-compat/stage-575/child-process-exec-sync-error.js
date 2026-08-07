const assert = require("assert");
const childProcess = require("child_process");

assert.throws(
  () => childProcess.execSync("does-not-exist"),
  (error) => error.status === 127,
);
assert.throws(
  () => childProcess.execFileSync("does-not-exist", ["arg"]),
  (error) =>
    error.code === "ENOENT" &&
    error.path === "does-not-exist" &&
    error.spawnargs[0] === "arg",
);

console.log("child process execSync errors passed");
