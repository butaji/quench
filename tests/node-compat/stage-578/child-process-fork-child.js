const assert = require("assert");
const childProcess = require("child_process");

assert.strictEqual(typeof childProcess._forkChild, "function");
assert.strictEqual(childProcess._forkChild.length, 2);

console.log("child process fork child passed");
