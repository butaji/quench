const assert = require("node:assert");
const crypto = require("node:crypto");

for (const name of ["modp5", "modp18"]) {
  const group = crypto.getDiffieHellman(name);
  assert.strictEqual(group.getPrime().byteLength, 128);
  assert.deepStrictEqual([...group.getGenerator()], [2]);
}
console.log("crypto named DH groups passed");
