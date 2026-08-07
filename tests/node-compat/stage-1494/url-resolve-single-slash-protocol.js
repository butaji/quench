const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve("http://example.com/b//c//d;p?q#blarg", "https:/p/a/t/h?s#hash2"),
  "https://p/a/t/h?s#hash2",
);
console.log("url resolve single slash protocol passed");
