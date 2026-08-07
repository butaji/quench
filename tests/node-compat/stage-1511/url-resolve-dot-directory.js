const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve("http://a/b/c/d;p?q", "./g/."),
  "http://a/b/c/g/",
);
console.log("url resolve dot directory passed");
