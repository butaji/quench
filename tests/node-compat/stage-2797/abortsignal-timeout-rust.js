'use strict';
const assert = require('assert');
const { once } = require('node:events');
const signal = AbortSignal.timeout(1);
assert.strictEqual(typeof signal, 'object');
signal.addEventListener('abort', () => assert.strictEqual(signal.aborted, true));
once(signal, 'abort').then(() => assert.strictEqual(signal.aborted, true));
const race = Promise.race([
  once(signal, 'abort').then(() => { throw signal.reason; }),
  new Promise((resolve) => setTimeout(resolve, 10)),
]);
race.catch((reason) => assert.strictEqual(reason.name, 'TimeoutError'));
setTimeout(() => {
  assert.strictEqual(signal.aborted, true);
  assert.strictEqual(signal.reason.name, 'TimeoutError');
}, 10);
