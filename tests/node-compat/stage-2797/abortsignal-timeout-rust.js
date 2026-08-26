'use strict';
const assert = require('assert');
const { once } = require('node:events');
const signal = AbortSignal.timeout(9000);
assert.strictEqual(typeof signal, 'object');
signal.addEventListener('abort', () => assert.strictEqual(signal.aborted, true));
once(signal, 'abort').then(() => assert.strictEqual(signal.aborted, true));
const race = Promise.race([
  once(signal, 'abort').then(() => { throw signal.reason; }),
  new Promise((resolve) => setTimeout(resolve, 10)),
]);
race.catch((reason) => assert.strictEqual(reason.name, 'TimeoutError'));
assert.rejects(
  () => Promise.race([
    once(signal, 'abort').then(() => { throw signal.reason; }),
    new Promise((resolve) => setTimeout(resolve, 10)),
  ]),
  { name: 'TimeoutError', message: 'The operation was aborted due to timeout' },
);
setTimeout(() => {
  assert.strictEqual(signal.aborted, true);
  assert.strictEqual(signal.reason.name, 'TimeoutError');
}, 10000);
