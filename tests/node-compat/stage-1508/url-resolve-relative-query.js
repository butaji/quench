const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve("http://a/b/c/d;p?q", "g?y"),
  "http://a/b/c/g?y",
);
console.log("url resolve relative query passed");
