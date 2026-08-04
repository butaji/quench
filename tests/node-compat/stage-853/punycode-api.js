"use strict";

const assert = require("assert");
const punycode = require("node:punycode");

for (const name of ["decode", "encode", "toASCII", "toUnicode"]) {
  assert.strictEqual(typeof punycode[name], "function");
}
assert.strictEqual(typeof punycode.ucs2.decode, "function");
assert.strictEqual(typeof punycode.ucs2.encode, "function");
assert.strictEqual(punycode.toASCII("mañana.com"), "xn--maana-pta.com");

console.log("punycode api passed");
