const assert = require("node:assert");
const crypto = require("node:crypto");

const prime = crypto.generatePrimeSync(32);
assert(Buffer.isBuffer(prime));
assert.strictEqual(crypto.checkPrimeSync(prime), true);
assert.throws(() => crypto.generatePrimeSync(0), { code: "ERR_OUT_OF_RANGE" });
console.log("crypto prime API passed");
