const assert = require("assert");
const util = require("util");

for (const invalidCode of [1, true, false, null, {}]) {
  assert.throws(() => util.deprecate(() => {}, "message", invalidCode), {
    code: "ERR_INVALID_ARG_TYPE",
    name: "TypeError",
    message: /The "code" argument must be of type string\./,
  });
}
