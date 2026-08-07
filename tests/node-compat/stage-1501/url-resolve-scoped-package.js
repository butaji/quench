const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve("https://registry.npmjs.org", "@foo/bar"),
  "https://registry.npmjs.org/@foo/bar",
);
console.log("url resolve scoped package passed");
