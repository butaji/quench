const assert = require("assert");
const events = require("events");

assert.throws(
  () => events.setMaxListeners("3"),
  (error) => error.code === "ERR_INVALID_ARG_TYPE",
);
assert.throws(
  () => events.setMaxListeners(-1),
  (error) => error.code === "ERR_OUT_OF_RANGE",
);
assert.throws(
  () => events.setMaxListeners(1, {}),
  (error) => error.code === "ERR_INVALID_ARG_TYPE",
);

const emitter = new events.EventEmitter();
assert.strictEqual(events.setMaxListeners(3, emitter), emitter);
assert.strictEqual(events.getMaxListeners(emitter), 3);
