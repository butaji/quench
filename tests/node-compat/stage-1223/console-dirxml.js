const assert = require("assert");

assert.strictEqual(typeof console.dirxml, "function");
assert.doesNotThrow(() => console.dirxml({ value: true }));
