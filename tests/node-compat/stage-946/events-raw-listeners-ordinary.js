const assert = require("assert");
const events = require("events");

const emitter = new events.EventEmitter();
const listener = () => {};
emitter.on("ready", listener);

const raw = emitter.rawListeners("ready");
assert.deepStrictEqual(raw, [listener]);
raw.length = 0;
assert.strictEqual(emitter.listenerCount("ready"), 1);
