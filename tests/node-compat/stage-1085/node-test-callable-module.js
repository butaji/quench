const assert = require("assert");
const test = require("node:test");

assert.strictEqual(typeof test, "function");
assert.strictEqual(test.test, test);
assert.strictEqual(test.it, test.describe);
