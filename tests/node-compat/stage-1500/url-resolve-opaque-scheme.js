const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("foo:a/b", "../c"), "foo:c");
console.log("url resolve opaque scheme passed");
