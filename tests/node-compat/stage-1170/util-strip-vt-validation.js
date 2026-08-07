const assert = require("assert");
const util = require("util");

assert.throws(() => util.stripVTControlCharacters({}), {
  code: "ERR_INVALID_ARG_TYPE",
  message:
    'The "str" argument must be of type string. Received an instance of Object',
});
