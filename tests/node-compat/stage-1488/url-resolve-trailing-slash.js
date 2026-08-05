const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("/foo/", "."), "/foo/");
console.log("url resolve trailing slash passed");
