const assert = require("node:assert");
const crypto = require("node:crypto");

const group = crypto.DiffieHellmanGroup("modp5");
assert(group instanceof crypto.DiffieHellmanGroup);
const ecdh = crypto.ECDH("prime256v1");
assert(ecdh instanceof crypto.ECDH);
console.log("crypto DH constructors passed");
