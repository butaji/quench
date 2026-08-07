const assert = require("node:assert");
const url = require("node:url");

const parsed = new URL("http://example.com/path");
for (const options of [true, 1, "test", Infinity, null]) {
  assert.throws(() => url.format(parsed, options), {
    code: "ERR_INVALID_ARG_TYPE",
    name: "TypeError",
  });
}
console.log("url format options validation passed");
