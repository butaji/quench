const assert = require("node:assert");
const crypto = require("node:crypto");

assert.strictEqual(typeof crypto.constants.RSA_PKCS1_PADDING, "number");
assert.strictEqual(typeof crypto.constants.RSA_PKCS1_PSS_PADDING, "number");
console.log("crypto RSA constants passed");
