const assert = require("node:assert");
const crypto = require("node:crypto");

const pair = crypto.generateKeyPairSync("ec", { namedCurve: "P-256" });
assert.doesNotThrow(() =>
  crypto.diffieHellman({
    privateKey: pair.privateKey,
    publicKey: pair.publicKey,
  })
);
console.log("crypto DH encoding variants passed");
