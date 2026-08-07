const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve("f://example.org/base/a", "b/c//d/e"),
  "f://example.org/base/b/c//d/e",
);
console.log("web double-slash resolution passed");
