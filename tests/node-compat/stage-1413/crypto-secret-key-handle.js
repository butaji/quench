const assert = require("node:assert");
const crypto = require("node:crypto");

const key = crypto.generateKeySync("aes", { length: 128 });
assert.strictEqual(key.type, "secret");
assert.strictEqual(key.export().byteLength, 16);
console.log("crypto secret-key handle passed");
