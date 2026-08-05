const assert = require("node:assert");
const crypto = require("node:crypto");

const dh = crypto.createDiffieHellman(1024);
assert(dh instanceof crypto.DiffieHellman);
console.log("crypto DH class identity passed");
