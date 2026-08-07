const assert = require("node:assert");
const childProcess = require("node:child_process");

const child = childProcess.spawn(process.execPath, [
  "fixture.js",
  "you-are-the-child",
]);
child.on("exit", (code) => assert.strictEqual(code, 0));

console.log("child process environment fixture passed");
