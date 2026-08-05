const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("foo:a/b?c#d", ""), "foo:a/b?c");
console.log("empty opaque resolution passed");
