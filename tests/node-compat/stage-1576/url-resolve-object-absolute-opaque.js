const assert = require("node:assert");
const url = require("node:url");

assert.deepStrictEqual(
  url.resolveObject(url.parse("http://a/b/c/d;p?q"), "g:h"),
  url.parse("g:h"),
);
console.log("parsed absolute opaque target passed");
