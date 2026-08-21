const assert = require("assert");
const events = require("events");

const emitter = new events.EventEmitter();
const first = () => {};
const second = () => {};
emitter.on("event", first);
emitter.on("event", second);
assert.strictEqual(events.listenerCount(emitter, "event"), 2);
assert.strictEqual(events.listenerCount(emitter, "event", first), 1);
