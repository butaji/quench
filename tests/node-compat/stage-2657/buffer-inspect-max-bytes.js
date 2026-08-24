"use strict";

const assert = require("assert");
const buffer = require("buffer");

assert.strictEqual(typeof buffer.INSPECT_MAX_BYTES, "number");
assert.throws(() => {
  buffer.INSPECT_MAX_BYTES = -1;
}, { code: "ERR_OUT_OF_RANGE" });
assert.throws(() => {
  buffer.INSPECT_MAX_BYTES = "50";
}, { code: "ERR_INVALID_ARG_TYPE" });
