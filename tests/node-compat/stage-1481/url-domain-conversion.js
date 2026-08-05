const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(typeof url.domainToASCII, "function");
assert.strictEqual(typeof url.domainToUnicode, "function");
assert.strictEqual(url.domainToASCII("новини.com"), "xn--b1amarcd.com");
console.log("url domain conversion passed");
