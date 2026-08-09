const assert = require("assert");
const common = require("../../node/common");
const { parse } = require("url");

const values = [undefined, null, true, 0, [], {}, () => {}, 1n, Symbol("x")];
for (const value of values) {
  const suffix = common.invalidArgTypeHelper(value);
  assert.throws(() => parse(value), {
    code: "ERR_INVALID_ARG_TYPE",
    name: "TypeError",
    message: `The "url" argument must be of type string.${suffix}`
  });
}

assert.strictEqual(
  common.invalidArgTypeHelper(() => {}),
  " Received function "
);
assert.strictEqual(
  common.invalidArgTypeHelper(1n),
  " Received type bigint (1n)"
);
