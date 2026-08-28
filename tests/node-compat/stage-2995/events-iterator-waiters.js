'use strict';

const assert = require('assert');
const { EventEmitter, on } = require('events');

(async () => {
  const emitter = new EventEmitter();
  const iterator = on(emitter, 'value');
  process.nextTick(() => {
    emitter.emit('value', 1);
    emitter.emit('value', 2);
    iterator.return();
  });

  assert.deepStrictEqual(await Promise.all([
    iterator.next(),
    iterator.next(),
    iterator.next(),
  ]), [
    { value: [1], done: false },
    { value: [2], done: false },
    { value: undefined, done: true },
  ]);
  assert.strictEqual(emitter.listenerCount('value'), 0);
  console.log('ok');
})();
