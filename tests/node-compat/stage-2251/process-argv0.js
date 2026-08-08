const assert = require("assert");

assert.strictEqual(process.argv[0], "quench-node");
assert.strictEqual(process.argv0, "node");
console.log("process argv0 passed");
