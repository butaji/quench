"use strict";

const assert = require("assert");
const punycode = require("punycode");

assert.strictEqual(punycode.toASCII("mañana.com"), "xn--maana-pta.com");
assert.strictEqual(punycode.toUnicode("xn--maana-pta.com"), "mañana.com");
assert.deepStrictEqual(punycode.ucs2.decode("😀"), [0x1f600]);
assert.strictEqual(punycode.ucs2.encode([0x1f600]), "😀");
assert.strictEqual(punycode.version, "2.1.0");

console.log("punycode passed");
