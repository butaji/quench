const assert = require("node:assert");
const url = require("node:url");

const base = "http://example.com/b//c//d;p?q#blarg";
assert.strictEqual(
  url.resolve(base, "http:#hash2"),
  "http://example.com/b//c//d;p?q#hash2",
);
console.log("url resolve same protocol hash passed");
