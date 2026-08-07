const assert = require("assert");
const { pipeline } = require("stream");

assert.throws(() => pipeline(), { code: "ERR_INVALID_ARG_TYPE" });
assert.throws(() => pipeline({}), { code: "ERR_MISSING_ARGS" });
assert.throws(() => pipeline({}, () => {}), { code: "ERR_MISSING_ARGS" });

const destination = {};
assert.strictEqual(
  pipeline({}, destination, () => {}),
  destination,
);
