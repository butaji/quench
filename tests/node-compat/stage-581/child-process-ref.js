const assert = require("assert");
const child = require("child_process").spawn("node", []);

assert.strictEqual(typeof child.ref, "function");
assert.strictEqual(typeof child.unref, "function");
assert.strictEqual(child.ref(), undefined);
assert.strictEqual(child.unref(), child);

console.log("child process ref passed");
