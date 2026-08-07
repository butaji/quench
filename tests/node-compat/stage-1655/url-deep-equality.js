const assert = require("node:assert");
assert.deepStrictEqual(
  new URL("./foo", "https://example.com/"),
  new URL("https://example.com/foo"),
);
console.log("URL deep equality passed");
