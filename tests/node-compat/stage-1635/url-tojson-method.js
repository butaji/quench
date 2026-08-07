const assert = require("node:assert");
const { URL } = require("node:url");

const descriptor = Object.getOwnPropertyDescriptor(URL.prototype, "toJSON");
assert.strictEqual(descriptor.enumerable, true);
assert.strictEqual(descriptor.value.name, "toJSON");
assert.strictEqual(
  new URL("https://example.com").toJSON(),
  "https://example.com/",
);
console.log("URL toJSON method passed");
