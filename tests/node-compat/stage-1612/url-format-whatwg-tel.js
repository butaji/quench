const assert = require("node:assert");
const url = require("node:url");

const value = new URL("tel:123");
assert.strictEqual(url.format(value), "tel:123");
assert.strictEqual(url.format(value, { unicode: true }), "tel:123");
console.log("WHATWG tel format matrix passed");
