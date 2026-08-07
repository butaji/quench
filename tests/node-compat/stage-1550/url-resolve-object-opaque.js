const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.format(url.resolveObject(url.parse("foo:a/b"), "/c/d")),
  "foo:/c/d",
);
console.log("parsed opaque resolution passed");
