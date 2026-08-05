const assert = require("node:assert");
const crypto = require("node:crypto");

const group = crypto.getDiffieHellman("modp14");
assert.doesNotThrow(() =>
  crypto.createDiffieHellman(group.getPrime(), group.getGenerator())
);
console.log("crypto valid DH generator passed");
