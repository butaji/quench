'use strict';
const assert = require('assert');
const order = [];
process.once('uncaughtException', (error) => {
  assert.strictEqual(error.message, 'boom');
  assert.deepStrictEqual(order, [0, 0, 0]);
});
for (let i = 0; i < 3; i++) setImmediate(() => order.push(0));
setImmediate(() => { throw new Error('boom'); });
