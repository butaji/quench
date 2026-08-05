const assert = require("node:assert");
const url = require("node:url");

const parsed = url.parse("http://www.日本語.com/");
assert.strictEqual(parsed.host, "www.xn--wgv71a119e.com");
assert.strictEqual(parsed.hostname, "www.xn--wgv71a119e.com");
console.log("legacy Unicode URL hosts passed");
