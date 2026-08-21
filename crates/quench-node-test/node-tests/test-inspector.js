// Node compat: inspector lifecycle and console surface (without transport).
const assert = require('node:assert');
const inspector = require('node:inspector');
const te = require('node:trace_events');
assert.strictEqual(typeof inspector, 'object');
assert.strictEqual(typeof te, 'object');
for (const name of ['open', 'close', 'url', 'waitForDebugger']) {
  assert.strictEqual(typeof inspector[name], 'function', `inspector.${name}`);
}
assert.strictEqual(typeof inspector.open, 'function');
assert.strictEqual(typeof inspector.url, 'function');
assert.strictEqual(typeof inspector.waitForDebugger, 'function');
assert.strictEqual(typeof inspector.close, 'function');
console.log('inspector+te: ok');
