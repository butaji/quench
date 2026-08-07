const assert = require("assert");
const events = require("events");

assert.throws(
  () => events.addAbortListener({}, () => {}),
  (error) => error.code === "ERR_INVALID_ARG_TYPE",
);
assert.throws(
  () => events.addAbortListener(new AbortController().signal, null),
  (error) => error.code === "ERR_INVALID_ARG_TYPE",
);
