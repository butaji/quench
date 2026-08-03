const assert = require("assert");
const childProcess = require("child_process");

assert.strictEqual(typeof childProcess.ChildProcess, "function");
const child = childProcess.spawn("node", []);
assert.strictEqual(child instanceof childProcess.ChildProcess, true);

console.log("child process constructor passed");
