'use strict';
const assert = require('assert');
const timers = require('timers');
const timerPromises = require('timers/promises');
const { promisify } = require('util');

const setPromiseImmediate = promisify(timers.setImmediate);
assert.strictEqual(setPromiseImmediate, timerPromises.setImmediate);

let calls = 0;
setPromiseImmediate().then((value) => {
  calls++;
  assert.strictEqual(value, undefined);
});
setPromiseImmediate('foobar').then((value) => {
  calls++;
  assert.strictEqual(value, 'foobar');
});
setTimeout(() => assert.strictEqual(calls, 2), 10);
