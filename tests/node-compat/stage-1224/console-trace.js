const assert = require("assert");

assert.strictEqual(typeof console.trace, "function");
assert.doesNotThrow(() => console.trace("trace"));
