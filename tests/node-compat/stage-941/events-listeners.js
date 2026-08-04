const assert = require("assert");
const events = require("events");

const emitter = new events.EventEmitter();
const listener = () => {};
emitter.on("ready", listener);

const listeners = emitter.listeners("ready");
assert.deepStrictEqual(listeners, [listener]);
listeners.length = 0;
assert.strictEqual(emitter.listenerCount("ready"), 1);
