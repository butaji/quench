const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve("http://a/b/c/d;p?q", "http:g"),
  "http://a/b/c/g",
);
console.log("url resolve same web scheme passed");
