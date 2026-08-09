const assert = require("assert");

assert.strictEqual(process.argv[0], process.execPath);
assert.strictEqual(process.argv0, "node");
assert.ok(process.execPath.endsWith("quench-node"));
