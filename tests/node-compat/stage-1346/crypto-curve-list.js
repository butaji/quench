const assert = require("node:assert");
const crypto = require("node:crypto");

assert.deepStrictEqual(crypto.getCurves(), ["secp384r1"]);
console.log("crypto curve list passed");
