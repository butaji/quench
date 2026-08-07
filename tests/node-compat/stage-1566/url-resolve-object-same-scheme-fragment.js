const assert = require("node:assert");
const url = require("node:url");

assert.deepStrictEqual(
  url.resolveObject(
    url.parse("http://example.com/b//c//d;p?q#blarg"),
    "http:#hash2",
  ),
  url.parse("http://example.com/b//c//d;p?q#hash2"),
);
console.log("same-scheme fragment resolution passed");
