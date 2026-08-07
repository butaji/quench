const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve("https://user:password@example.com", "https://example.com/foo"),
  "https://user:password@example.com/foo",
);
console.log("authenticated target resolution passed");
