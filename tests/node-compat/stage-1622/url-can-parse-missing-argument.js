const assert = require("node:assert");
const { URL } = require("node:url");

assert.throws(() => URL.canParse(), {
  code: "ERR_MISSING_ARGS",
  name: "TypeError",
});
assert.strictEqual(URL.canParse("https://example.com"), true);
console.log("URL.canParse missing argument passed");
