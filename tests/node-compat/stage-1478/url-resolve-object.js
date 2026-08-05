const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolveObject("", "foo"), "foo");
assert.strictEqual(url.resolveObject("/foo/bar/baz", "quux"), "/foo/bar/quux");
console.log("url resolveObject passed");
