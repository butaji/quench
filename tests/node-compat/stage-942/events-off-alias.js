const assert = require("assert");
const events = require("events");

const emitter = new events.EventEmitter();
const listener = () => {};
assert.strictEqual(emitter.off, emitter.removeListener);

emitter.on("ready", listener);
assert.strictEqual(emitter.listenerCount("ready"), 1);
emitter.off("ready", listener);
assert.strictEqual(emitter.listenerCount("ready"), 0);
