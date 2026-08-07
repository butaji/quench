const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve("http://example.com/b//c//d;p?q#blarg", "https:#hash2"),
  "https:///#hash2",
);
console.log("url resolve protocol hash passed");
