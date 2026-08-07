const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve("#Animal", "file:/swap/test/animal.rdf"),
  "file:///swap/test/animal.rdf#Animal",
);
console.log("file URL fragment resolution passed");
