'use strict';

const assert = require('assert');

for (const [name, value] of Object.entries({
  NONE: 0,
  CAPTURING_PHASE: 1,
  AT_TARGET: 2,
  BUBBLING_PHASE: 3,
})) {
  assert.strictEqual(Event[name], value);
  const descriptor = Object.getOwnPropertyDescriptor(Event, name);
  assert.strictEqual(descriptor.writable, false);
  assert.strictEqual(descriptor.configurable, false);
  assert.strictEqual(descriptor.enumerable, true);
}

console.log('ok');
