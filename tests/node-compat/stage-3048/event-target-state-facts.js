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

const identityTarget = new EventTarget();
const identityEvent = new Event('identity');
assert.strictEqual(identityEvent.target, null);
assert.strictEqual(identityEvent.currentTarget, null);
const identities = [];
identityTarget.addEventListener('identity', function(event) {
  identities.push([event === identityEvent, event.target === identityTarget,
    event.currentTarget === identityTarget, this === identityTarget]);
}, { once: true });
identityTarget.addEventListener('identity', function(event) {
  identities.push([event === identityEvent, event.target === identityTarget,
    event.currentTarget === null, this === identityTarget]);
});
identityTarget.dispatchEvent(identityEvent);
assert.deepStrictEqual(identities, [[true, true, true, true], [true, true, true, true]]);
assert.strictEqual(identityEvent.target, identityTarget);
assert.strictEqual(identityEvent.currentTarget, null);
freshTarget.dispatchEvent(identityEvent);
assert.strictEqual(identityEvent.target, freshTarget);
assert.strictEqual(identityEvent.currentTarget, null);
for (const name of ['target', 'currentTarget']) {
  const descriptor = Object.getOwnPropertyDescriptor(Event.prototype, name);
  assert.strictEqual(typeof descriptor.get, 'function');
  assert.strictEqual(descriptor.set, undefined);
  assert.strictEqual(descriptor.enumerable, true);
  assert.strictEqual(descriptor.configurable, true);
  assert.strictEqual(Object.hasOwn(identityEvent, name), false);
  for (const receiver of [{}, Object.create(Event.prototype)]) {
    assert.throws(() => descriptor.get.call(receiver), {
      name: 'TypeError', code: 'ERR_INVALID_THIS',
      message: 'Value of "this" must be of type Event',
    });
  }
}

const controller = new AbortController();
let abortEvent;
controller.signal.addEventListener('abort', function(event) {
  abortEvent = event;
  assert.strictEqual(event.target, controller.signal);
  assert.strictEqual(event.currentTarget, controller.signal);
  assert.strictEqual(this, controller.signal);
});
controller.abort();
assert.strictEqual(abortEvent.target, controller.signal);
assert.strictEqual(abortEvent.currentTarget, null);

const recursiveTarget = new EventTarget();
const recursiveEvent = new Event('recursive');
recursiveTarget.addEventListener('recursive', () => {
  assert.throws(() => recursiveTarget.dispatchEvent(recursiveEvent), {
    name: 'Error', code: 'ERR_EVENT_RECURSION',
    message: 'The event "recursive" is already being dispatched',
  });
}, { once: true });
recursiveTarget.dispatchEvent(recursiveEvent);
assert.strictEqual(recursiveEvent.currentTarget, null);
assert.throws(() => recursiveTarget.dispatchEvent(), {
  name: 'TypeError', code: 'ERR_MISSING_ARGS',
  message: 'The "event" argument must be specified',
});
assert.throws(() => EventTarget.prototype.dispatchEvent.call({}, recursiveEvent), {
  name: 'TypeError', code: 'ERR_INVALID_THIS',
  message: 'Value of "this" must be of type EventTarget',
});
assert.throws(() => recursiveTarget.dispatchEvent({}), {
  name: 'TypeError', code: 'ERR_INVALID_ARG_TYPE',
  message: 'The "event" argument must be an instance of Event. Received an instance of Object',
});
