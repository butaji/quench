const assert = require("node:assert");
const url = new URL("http://example.com/path");
assert.throws(
  () => {
    url.href = "";
  },
  { name: "TypeError" },
);
assert.strictEqual(url.href, "http://example.com/path");
console.log("Empty URL href rejected");
