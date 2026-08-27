const assert = require("assert");
const { EventEmitter, on } = require("events");

(async () => {
  const emitter = new EventEmitter();
  const error = new Error("boom");
  const iterator = on(emitter, "value");
  process.nextTick(() => emitter.emit("error", error));
  await assert.rejects(iterator.next(), (reason) => reason === error);
  assert.strictEqual(emitter.listenerCount("value"), 0);
  assert.strictEqual(emitter.listenerCount("error"), 0);

  const delayed = new EventEmitter();
  const delayedError = new Error("delayed");
  const delayedIterator = on(delayed, "value");
  process.nextTick(() => {
    delayed.emit("value", 1);
    delayed.emit("error", delayedError);
  });
  assert.deepStrictEqual(await delayedIterator.next(), { value: [1], done: false });
  await assert.rejects(delayedIterator.next(), (reason) => reason === delayedError);
  assert.strictEqual(delayed.listenerCount("value"), 0);
  assert.strictEqual(delayed.listenerCount("error"), 0);

  const thrownEmitter = new EventEmitter();
  const thrownIterator = on(thrownEmitter, "value");
  assert.throws(() => thrownIterator.throw(), TypeError);
  const thrown = new Error("thrown");
  await assert.rejects(thrownIterator.throw(thrown), (reason) => reason === thrown);
  assert.strictEqual(thrownEmitter.listenerCount("value"), 0);
})();
