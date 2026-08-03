const assert = require("assert");
const before = process.umask();
assert.strictEqual(typeof before, "number");
const previous = process.umask(0o077);
assert.strictEqual(previous, before);
assert.strictEqual(process.umask(), 0o077);
process.umask(before);
