const assert = require("node:assert");
const url = require("node:url");

assert.deepStrictEqual(
  url.resolveObject(url.parse("https://registry.npmjs.org"), "@foo/bar"),
  url.parse("https://registry.npmjs.org/@foo/bar"),
);
console.log("parsed scoped path resolution passed");
