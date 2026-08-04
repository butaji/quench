const assert = require("assert");
const { EventEmitter, getEventListeners } = require("events");

const first = () => {};
const second = () => {};
const emitter = new EventEmitter();
emitter.on("foo", first);
emitter.on("foo", second);
assert.deepStrictEqual(getEventListeners(emitter, "foo"), [first, second]);
assert.deepStrictEqual(getEventListeners(emitter, "missing"), []);

const target = new EventTarget();
target.addEventListener("foo", first);
target.addEventListener("foo", second);
assert.deepStrictEqual(getEventListeners(target, "foo"), [first, second]);
assert.throws(() => getEventListeners("invalid"), /ERR_INVALID_ARG_TYPE/);
