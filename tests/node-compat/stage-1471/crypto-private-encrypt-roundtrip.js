const assert = require("node:assert");
const crypto = require("node:crypto");

const message = Buffer.from("I AM THE WALRUS");
const encrypted = crypto.privateEncrypt("key", message);
assert.deepStrictEqual(crypto.publicDecrypt("key", encrypted), message);
console.log("crypto private encryption passed");
