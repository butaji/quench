const assert = require("node:assert");
const crypto = require("node:crypto");

assert(crypto.createDiffieHellman(1024) instanceof crypto.DiffieHellman);
assert(
  crypto.createDiffieHellmanGroup("modp5") instanceof crypto.DiffieHellmanGroup,
);
assert(crypto.createECDH("prime256v1") instanceof crypto.ECDH);
console.log("crypto key exchange prototypes passed");
