const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("foo:a/b", "/c/d"), "foo:/c/d");
console.log("opaque absolute string resolution passed");
