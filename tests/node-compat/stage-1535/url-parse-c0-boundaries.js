const assert = require("node:assert");
const url = require("node:url");

const parsed = url.parse("\bhttp://example.com/\b");
assert.strictEqual(parsed.href, "http://example.com/");
assert.strictEqual(parsed.hostname, "example.com");
console.log("legacy URL C0 boundaries passed");
