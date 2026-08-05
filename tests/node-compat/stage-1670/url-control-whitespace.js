const assert = require("node:assert");

const url = new URL("http://example\t.\norg", "http://example.org/foo/bar");
assert.strictEqual(url.href, "http://example.org/");
console.log("URL control whitespace normalization passed");
