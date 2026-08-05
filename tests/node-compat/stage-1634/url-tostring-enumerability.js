const assert = require("node:assert");
const { URL } = require("node:url");

const descriptor = Object.getOwnPropertyDescriptor(URL.prototype, "toString");
assert.strictEqual(descriptor.enumerable, true);
assert.strictEqual(descriptor.writable, true);
console.log("URL toString enumerability passed");
