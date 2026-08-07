const assert = require("assert");
const { Readable } = require("stream");

(async () => {
  const stream = Readable.from([1, 2]);
  assert.strictEqual(typeof stream.iterator, "function");
  assert.throws(() => stream.iterator(42), {
    code: "ERR_INVALID_ARG_TYPE",
    message:
      'The "options" argument must be of type object. Received type number (42)',
  });
  const iterator = stream.iterator({ destroyOnReturn: false });
  assert.strictEqual((await iterator.next()).value, 1);
  await iterator.return();
  assert.strictEqual(stream.destroyed, false);

  const destroying = Readable.from([1, 2]);
  const destroyingIterator = destroying.iterator();
  await destroyingIterator.next();
  await destroyingIterator.return();
  assert.strictEqual(destroying.destroyed, true);
})();
