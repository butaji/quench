const assert = require("assert");
const child = require("child_process").spawn("node", []);

assert.strictEqual(typeof child.destroy, "function");
assert.strictEqual(typeof child[Symbol.dispose], "function");
assert.strictEqual(child.killed, false);
child[Symbol.dispose]();
assert.strictEqual(child.killed, true);

console.log("child process dispose passed");
