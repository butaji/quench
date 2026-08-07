const assert = require("node:assert");
const url = require("node:url");

assert.deepStrictEqual(
  url.resolveObject(
    url.parse("http://example.com/b//c//d;p?q#blarg"),
    "https:#hash2",
  ),
  url.parse("https:///#hash2"),
);
console.log("parsed cross-protocol resolution passed");
