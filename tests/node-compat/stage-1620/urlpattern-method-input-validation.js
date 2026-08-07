const assert = require("node:assert");
const { URLPattern } = require("node:url");

const pattern = new URLPattern();
for (const method of [pattern.exec, pattern.test]) {
  assert.throws(() => method(1), {
    code: "ERR_INVALID_ARG_TYPE",
    name: "TypeError",
  });
  assert.throws(() => method("", 1), {
    code: "ERR_INVALID_ARG_TYPE",
    name: "TypeError",
  });
}
console.log("URLPattern method input validation passed");
