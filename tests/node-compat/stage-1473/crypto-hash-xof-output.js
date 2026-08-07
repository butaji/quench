const assert = require("node:assert");
const crypto = require("node:crypto");

const value = crypto.hash("shake256", "test", {
  outputLength: 16,
  outputEncoding: "hex",
});
assert.strictEqual(typeof value, "string");
assert.strictEqual(value.length, 32);
console.log("crypto XOF hash output passed");
