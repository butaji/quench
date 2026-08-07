const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve("mailto:local@domain?query1", "?query2"),
  "mailto:local@domain?query2",
);
console.log("mailto query resolution passed");
