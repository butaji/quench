'use strict';
const assert = require('assert');
const { EventEmitter, on } = require('events');
const ac = new AbortController();
const emitter = new EventEmitter();
const pending = (async () => {
  for await (const value of on(emitter, 'value', { signal: ac.signal })) {
    assert.strictEqual(value[0], 1);
  }
})();
pending.catch((error) => {
  assert.strictEqual(error.name, 'AbortError');
});
process.nextTick(() => ac.abort());
const target = new EventTarget();
const targetController = new AbortController();
(async () => {
  for await (const event of on(target, 'tick', { signal: targetController.signal })) {
    assert.strictEqual(event[0].type, 'tick');
  }
})().catch((error) => {
  assert.strictEqual(error.name, 'AbortError');
});
process.nextTick(() => targetController.abort());
