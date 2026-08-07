const assert = require("node:assert");
const url = require("node:url");

assert.deepStrictEqual(
  url.resolveObject(url.parse("http://a/b/c/d;p?q"), "http:"),
  url.parse("http://a/b/c/d;p?q"),
);
console.log("parsed empty scheme passed");
