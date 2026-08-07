const assert = require("node:assert");
const url = require("node:url");

assert.deepStrictEqual(
  url.resolveObject(url.parse("zz:abc"), "/foo/../../../bar"),
  url.parse("zz:/bar"),
);
console.log("parsed opaque absolute passed");
