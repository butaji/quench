const assert = require("node:assert");
const crypto = require("node:crypto");

for (const target of [new crypto.Certificate(), crypto.Certificate]) {
  for (const name of ["verifySpkac", "exportPublicKey", "exportChallenge"]) {
    assert.strictEqual(typeof target[name], "function");
  }
}
console.log("crypto certificate methods passed");
