const assert = require("node:assert");

const params = new URLSearchParams();
for (
  const [method, args] of [
    ["append", []],
    ["set", ["name"]],
    ["get", []],
    ["getAll", []],
    ["has", []],
    ["delete", []],
  ]
) {
  assert.throws(() => params[method](...args), {
    code: "ERR_MISSING_ARGS",
    name: "TypeError",
  });
}
assert.throws(() => params.append.call(undefined, "a", "b"), {
  code: "ERR_INVALID_THIS",
  name: "TypeError",
});
console.log("URLSearchParams argument validation passed");
