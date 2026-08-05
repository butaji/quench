const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("/foo/bar/", ".."), "/foo/");
console.log("url resolve parent directory passed");
