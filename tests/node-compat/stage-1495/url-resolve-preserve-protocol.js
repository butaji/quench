const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve("http://example.com", "https://u:p@h.com/p"),
  "https://u:p@h.com/p",
);
console.log("url resolve protocol preserved passed");
