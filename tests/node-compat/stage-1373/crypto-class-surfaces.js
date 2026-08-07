const assert = require("node:assert");
const crypto = require("node:crypto");

for (const name of ["Hmac", "Sign", "Verify", "DiffieHellmanGroup"]) {
  assert.strictEqual(typeof crypto[name], "function");
}
console.log("crypto class surfaces passed");
