const assert = require("assert");
const events = require("events");

const emitter = new events.EventEmitter();
const listener = () => {};
emitter.once("ready", listener);

assert.deepStrictEqual(emitter.listeners("ready"), [listener]);
const raw = emitter.rawListeners("ready");
assert.strictEqual(raw.length, 1);
assert.notStrictEqual(raw[0], listener);
assert.strictEqual(raw[0].listener, listener);
