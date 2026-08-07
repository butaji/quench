const assert = require("assert");
const events = require("events");

assert.throws(
  () => events.listenerCount({}, "event"),
  (error) => error.code === "ERR_INVALID_ARG_TYPE",
);
assert.throws(
  () => events.listenerCount(null, "event"),
  (error) => error.code === "ERR_INVALID_ARG_TYPE",
);
