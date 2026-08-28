'use strict';

const assert = require('assert');

// Event owns its durable cancellation state.  Dispatch bookkeeping must not
// leak an event's state into later events or retain identities in the host.
const target = new EventTarget();
const cancelable = new Event('cancel', { cancelable: true });
target.addEventListener('cancel', (event) => event.preventDefault());
assert.strictEqual(target.dispatchEvent(cancelable), false);
assert.strictEqual(cancelable.defaultPrevented, true);
const freshTarget = new EventTarget();
assert.strictEqual(freshTarget.dispatchEvent(new Event('cancel', { cancelable: true })), true);

const notCancelable = new Event('plain');
target.addEventListener('plain', (event) => event.preventDefault());
assert.strictEqual(target.dispatchEvent(notCancelable), true);
assert.strictEqual(notCancelable.defaultPrevented, false);

const order = [];
const immediate = new Event('immediate');
target.addEventListener('immediate', () => {
  order.push('first');
  immediate.stopImmediatePropagation();
});
target.addEventListener('immediate', () => order.push('second'));
assert.strictEqual(target.dispatchEvent(immediate), true);
assert.deepStrictEqual(order, ['first']);
