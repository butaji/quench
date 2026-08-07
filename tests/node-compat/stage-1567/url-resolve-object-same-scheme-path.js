const assert = require("node:assert");
const url = require("node:url");

assert.deepStrictEqual(
  url.resolveObject(
    url.parse("http://example.com/b//c//d;p?q#blarg"),
    "http:/p/a/t/h?s#hash2",
  ),
  url.parse("http://example.com/p/a/t/h?s#hash2"),
);
console.log("same-scheme parsed path passed");
