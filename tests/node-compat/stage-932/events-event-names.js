const assert = require("assert");
const EventEmitter = require("events");

const emitter = new EventEmitter();
const listener = () => {};
emitter.on("foo", listener);
assert.deepStrictEqual(emitter.eventNames(), ["foo"]);
emitter.on(Symbol.for("symbol-event"), listener);
assert.deepStrictEqual(emitter.eventNames(), [
  "foo",
  Symbol.for("symbol-event"),
]);
emitter.removeListener("foo", listener);
assert.deepStrictEqual(emitter.eventNames(), [Symbol.for("symbol-event")]);
