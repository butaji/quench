const assert = require("assert");
const events = require("events");

for (const target of [undefined, null, {}, 1]) {
  assert.throws(
    () => events.getMaxListeners(target),
    (error) => error.code === "ERR_INVALID_ARG_TYPE",
  );
}

assert.strictEqual(events.getMaxListeners(new events.EventEmitter()), 10);
assert.strictEqual(events.getMaxListeners(new EventTarget()), 10);
