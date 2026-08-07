const assert = require("node:assert");
const url = require("node:url");

for (
  const input of [undefined, null, true, false, 0, () => {}, Symbol("foo")]
) {
  assert.throws(() => url.format(input), {
    code: "ERR_INVALID_ARG_TYPE",
    name: "TypeError",
  });
}
assert.strictEqual(url.format(""), "");
assert.strictEqual(url.format({}), "");
console.log("url format input validation passed");
