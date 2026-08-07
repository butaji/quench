const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve("fred:///s//a/b/c", "../../../g"),
  "fred:///s/g",
);
console.log("double-slash parent resolution passed");
