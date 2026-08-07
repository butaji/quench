const assert = require("node:assert");
const url = require("node:url");

assert.deepStrictEqual(
  url.resolveObject(url.parse("foo:a/b"), "../c"),
  url.parse("foo:c"),
);
console.log("parsed opaque parent passed");
