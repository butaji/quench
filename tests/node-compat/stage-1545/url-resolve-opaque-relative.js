const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("c/d", "foo:a/b"), "foo:a/c/d");
console.log("opaque scheme relative resolution passed");
