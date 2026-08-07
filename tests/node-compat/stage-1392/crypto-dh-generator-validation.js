const assert = require("node:assert");
const crypto = require("node:crypto");

for (
  const generator of [
    -1,
    1,
    Buffer.alloc(0),
    Buffer.from([0]),
    Buffer.from([1]),
  ]
) {
  assert.throws(() => crypto.createDiffieHellman("abcdef", generator), {
    code: "ERR_OSSL_DH_BAD_GENERATOR",
  });
}
console.log("crypto DH generator validation passed");
