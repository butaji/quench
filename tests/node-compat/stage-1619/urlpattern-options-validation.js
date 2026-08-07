const assert = require("node:assert");
const { URLPattern } = require("node:url");

assert.throws(() => new URLPattern({}, 1), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
});
assert.strictEqual(new URLPattern({}, { ignoreCase: "" }).protocol, "*");
console.log("URLPattern options validation passed");
