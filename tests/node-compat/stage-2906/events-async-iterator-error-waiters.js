const assert = require("assert");
const { EventEmitter, on } = require("events");

(async () => {
  const emitter = new EventEmitter();
  const iterator = on(emitter, "value");
  const error = new Error("stop");
  const results = [iterator.next(), iterator.next(), iterator.next()];
  emitter.emit("error", error);
  assert.deepStrictEqual(await Promise.allSettled(results), [
    { status: "rejected", reason: error },
    { status: "fulfilled", value: { value: undefined, done: true } },
    { status: "fulfilled", value: { value: undefined, done: true } },
  ]);
})();
