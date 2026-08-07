const assert = require("assert");
const buffer = require("buffer");

for (const value of [NaN, -1]) {
  assert.throws(
    () => {
      buffer.INSPECT_MAX_BYTES = value;
    },
    {
      code: "ERR_OUT_OF_RANGE",
      name: "RangeError",
    },
  );
}

assert.throws(
  () => {
    buffer.INSPECT_MAX_BYTES = "and even this";
  },
  {
    code: "ERR_INVALID_ARG_TYPE",
    name: "TypeError",
  },
);
