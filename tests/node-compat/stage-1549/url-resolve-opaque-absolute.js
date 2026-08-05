const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("/c/d", "foo:a/b"), "foo:/c/d");
console.log("opaque absolute resolution passed");
