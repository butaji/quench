const assert = require("assert");
const common = require("../../node/test/common");
const crypto = require("crypto");

assert.throws(() => crypto.randomInt(NaN, 100), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
  message: 'The "min" argument must be a safe integer.' +
    common.invalidArgTypeHelper(NaN),
});
