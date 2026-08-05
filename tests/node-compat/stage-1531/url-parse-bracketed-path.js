const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.parse("[fe80::1]").pathname, "[fe80::1]");
assert.strictEqual(url.parse("[fe80::1]").href, "[fe80::1]");
console.log("bracketed legacy URL paths passed");
