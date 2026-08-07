const assert = require("node:assert");
const crypto = require("node:crypto");

for (
  const [name, args] of [
    ["DiffieHellman", [1024]],
    ["DiffieHellmanGroup", ["modp5"]],
    ["ECDH", ["prime256v1"]],
  ]
) {
  assert(crypto[name](...args) instanceof crypto[name]);
}
console.log("crypto DH callable constructors passed");
