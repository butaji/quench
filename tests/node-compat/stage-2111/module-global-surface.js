const assert = require("assert");

assert.strictEqual(module.parent, null);
assert.strictEqual(module.filename, globalThis.__filename);
assert.strictEqual(typeof require.cache, "object");
assert.strictEqual(typeof require.extensions, "object");

console.log("module global surface pass");
