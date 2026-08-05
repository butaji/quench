const assert = require("node:assert");
const { URL } = require("node:url");

const descriptor = Object.getOwnPropertyDescriptor(URL.prototype, "href");
assert.strictEqual(descriptor.enumerable, true);
assert.strictEqual(typeof descriptor.get, "function");
console.log("URL href enumerability passed");
