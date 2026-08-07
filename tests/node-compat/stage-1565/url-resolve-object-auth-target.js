const assert = require("node:assert");
const url = require("node:url");
assert.deepStrictEqual(
  url.resolveObject(
    url.parse("http://example.com/b//c//d;p?q#blarg"),
    "https://u:p@h.com/p/a/t/h?s#hash2",
  ),
  url.parse("https://u:p@h.com/p/a/t/h?s#hash2"),
);
console.log("parsed authenticated target passed");
