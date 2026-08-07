const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve("http://a/b/c/d;p?q", "?y"),
  "http://a/b/c/d;p?y",
);
console.log("url resolve query only passed");
