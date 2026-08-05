const assert = require("node:assert");
const crypto = require("node:crypto");

assert(crypto.Certificate() instanceof crypto.Certificate);
assert(new crypto.Certificate() instanceof crypto.Certificate);
console.log("crypto Certificate callable passed");
