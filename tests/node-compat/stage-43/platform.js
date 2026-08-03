const assert = require("assert");
assert.notStrictEqual(process.platform, "unknown");
assert.notStrictEqual(process.arch, "unknown");
assert.strictEqual(typeof process.platform, "string");
assert.strictEqual(typeof process.arch, "string");
