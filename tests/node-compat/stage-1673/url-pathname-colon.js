const assert = require("node:assert");

const url = new URL("\t   :foo.com   \n", "http://example.org/foo/bar");
assert.strictEqual(url.href, "http://example.org/foo/:foo.com");
console.log("URL pathname colon preservation passed");
