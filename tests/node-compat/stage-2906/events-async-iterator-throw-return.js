const assert = require("assert");
const { EventEmitter, on } = require("events");

(async () => {
  const emitter = new EventEmitter();
  const iterator = on(emitter, "value");
  const error = new Error("stop");
  assert.strictEqual(iterator.throw(error), undefined);
  assert.strictEqual(emitter.listenerCount("value"), 0);
  assert.strictEqual(emitter.listenerCount("error"), 0);
  assert.deepStrictEqual(await iterator.next(), {
    value: undefined,
    done: true,
  });
})();
