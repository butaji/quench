const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("foo:a", "."), "foo:");
console.log("opaque dot resolution passed");
