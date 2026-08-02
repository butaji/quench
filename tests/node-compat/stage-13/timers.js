const assert = require('assert');
const timers = require('node:timers');
const timersPromises = require('timers/promises');
let fired = false;
timers.setImmediate(() => { fired = true; });
timersPromises.setTimeout(0, 'done').then((value) => {
  assert.strictEqual(value, 'done');
  assert.strictEqual(fired, true);
});
