const assert = require("node:assert");
const { URL, parse } = require("node:url");
const { isURL } = require("internal/url");

assert.strictEqual(isURL(new URL("https://example.com")), true);
assert.strictEqual(isURL(parse("https://example.com")), false);
assert.strictEqual(isURL({ href: "https://example.com" }), false);
console.log("internal URL brand detection passed");
