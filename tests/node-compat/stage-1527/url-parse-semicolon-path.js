const assert = require("node:assert");
const url = require("node:url");

const parsed = url.parse("HtTp://x.y.cOm;a/b/c?d=e#f g<h>i");
assert.strictEqual(parsed.pathname, ";a/b/c");
assert.strictEqual(parsed.host, "x.y.com");
assert.strictEqual(parsed.href, "http://x.y.com/;a/b/c?d=e#f%20g%3Ch%3Ei");
console.log("legacy semicolon URL paths passed");
