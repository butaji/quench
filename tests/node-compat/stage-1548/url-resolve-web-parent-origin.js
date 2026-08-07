const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve("http://example.com/a/b", "../c"),
  "http://example.com/c",
);
console.log("web parent origin resolution passed");
