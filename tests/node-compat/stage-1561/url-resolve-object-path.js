const assert = require("node:assert");
const url = require("node:url");
assert.deepStrictEqual(
  url.resolveObject(url.parse("/foo/bar/baz"), "quux"),
  url.parse("/foo/bar/quux"),
);
console.log("parsed relative path resolution passed");
