const assert = require("assert");
assert.strictEqual(typeof process.execPath, "string");
assert.strictEqual(process.execPath.length > 0, true);
assert.strictEqual(process.argv[0], process.execPath);
