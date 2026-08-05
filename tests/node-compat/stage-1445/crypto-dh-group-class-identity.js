const assert = require("node:assert");
const crypto = require("node:crypto");

const group = crypto.createDiffieHellmanGroup("modp5");
assert(group instanceof crypto.DiffieHellmanGroup);
console.log("crypto DH group class identity passed");
