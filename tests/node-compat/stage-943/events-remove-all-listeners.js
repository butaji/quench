const assert = require("assert");
const events = require("events");

const emitter = new events.EventEmitter();
const first = () => {};
const second = () => {};
emitter.on("ready", first).on("close", second);

assert.strictEqual(emitter.removeAllListeners("ready"), emitter);
assert.strictEqual(emitter.listenerCount("ready"), 0);
assert.strictEqual(emitter.listenerCount("close"), 1);
assert.strictEqual(emitter.removeAllListeners(), emitter);
assert.deepStrictEqual(emitter.eventNames(), []);
