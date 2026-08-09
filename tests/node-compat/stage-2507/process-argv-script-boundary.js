const assert = require("assert");

assert.strictEqual(process.argv[0], process.execPath);
assert.strictEqual(process.argv[1], __filename);
assert.strictEqual(process.argv[2], undefined);
