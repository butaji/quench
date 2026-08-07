const assert = require("assert");
const { formatWithOptions } = require("util");

for (const value of [undefined, null, false, 5, "test"]) {
  assert.throws(() => formatWithOptions(value, { a: true }), {
    code: "ERR_INVALID_ARG_TYPE",
    message: /"inspectOptions".+object/,
  });
}
