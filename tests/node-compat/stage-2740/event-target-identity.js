const assert = require('assert');
const { EventTarget, CustomEvent } = require('internal/event_target');

const target = new EventTarget();
const event = new CustomEvent('value', { detail: { ok: true } });
let received;
target.addEventListener('value', (current) => { received = current; });
assert.strictEqual(target.dispatchEvent(event), true);
assert.strictEqual(received, event);
assert.strictEqual(event.target, target);
assert.strictEqual(event.currentTarget, null);
assert.deepStrictEqual(event.detail, { ok: true });
