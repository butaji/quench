const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve("http://example.com", "https://u:p@h.com/p?s#hash2"),
  "https://u:p@h.com/p?s#hash2",
);
console.log("url resolve absolute query passed");
