const assert = require("assert");
const { atob, btoa } = require("node:buffer");
assert.strictEqual(btoa("hello"), "aGVsbG8=");
assert.strictEqual(atob("aGVsbG8="), "hello");
