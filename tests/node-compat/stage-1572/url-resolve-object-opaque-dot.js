const assert = require("node:assert");
const url = require("node:url");

assert.deepStrictEqual(
  url.resolveObject(url.parse("foo:a"), "foo:."),
  url.parse("foo:"),
);
console.log("parsed opaque dot passed");
