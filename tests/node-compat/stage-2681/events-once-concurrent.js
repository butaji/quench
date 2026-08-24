const assert = require('assert');
const { once, EventEmitter } = require('events');

async function event() {
  const ee = new EventEmitter();
  process.nextTick(() => ee.emit('event', 42));
  assert.deepStrictEqual(await once(ee, 'event'), [42]);
}
async function error() {
  const ee = new EventEmitter(); const expected = new Error('boom');
  process.nextTick(() => ee.emit('error', expected));
  await assert.rejects(once(ee, 'event'), (value) => value === expected);
}
async function abort() {
  const ee = new EventEmitter(); const controller = new AbortController();
  const pending = once(ee, 'event', { signal: controller.signal });
  process.nextTick(() => controller.abort());
  await assert.rejects(pending, { name: 'AbortError' });
}
async function target() {
  const target = new EventTarget();
  process.nextTick(() => target.dispatchEvent(new Event('event')));
  const [event] = await once(target, 'event');
  assert.strictEqual(event.type, 'event');
}
async function invalid() {
  const ee = new EventEmitter();
  await Promise.all([1, 'hi', null, false, () => {}, Symbol(), 1n].map((value) =>
    assert.rejects(once(ee, 'event', value), { code: 'ERR_INVALID_ARG_TYPE' }),
  ));
}
Promise.all([event(), error(), abort(), target(), invalid()]).catch((error) => {
  throw error;
});
