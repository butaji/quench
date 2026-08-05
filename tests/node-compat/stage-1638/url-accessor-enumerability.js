const assert = require("node:assert");
const { URL } = require("node:url");

const descriptor = Object.getOwnPropertyDescriptor(URL.prototype, "href");
assert.strictEqual(descriptor.enumerable, true);
console.log("URL accessor enumerability passed");
