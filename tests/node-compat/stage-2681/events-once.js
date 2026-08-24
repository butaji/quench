const assert = require('assert');
const { once, EventEmitter } = require('events');

(async () => {
  const ee = new EventEmitter();
  process.nextTick(() => ee.emit('event', 42, 24));
  assert.deepStrictEqual(await once(ee, 'event'), [42, 24]);
  assert.strictEqual(ee.listenerCount('event'), 0);

  const errors = new EventEmitter();
  const expected = new Error('boom');
  process.nextTick(() => errors.emit('error', expected));
  await assert.rejects(once(errors, 'event'), (error) => error === expected);
  assert.strictEqual(errors.listenerCount('event'), 0);
  assert.strictEqual(errors.listenerCount('error'), 0);

  await assert.rejects(once(new EventEmitter(), 'event', null), {
    code: 'ERR_INVALID_ARG_TYPE',
  });

  const target = new EventTarget();
  target.dispatchEvent(new Event('event'));
  await assert.rejects(once(target, 'event', { signal: 1 }), {
    code: 'ERR_INVALID_ARG_TYPE',
  });
  const abort = new AbortController();
  const pending = once(target, 'later', { signal: abort.signal });
  abort.abort();
  await assert.rejects(pending, { name: 'AbortError' });

  const ee2 = new EventEmitter();
  ee2.on('error', () => assert.fail('error listener called'));
  const already = AbortSignal.abort();
  await Promise.all([1, {}, 'hi', null, false].map((signal) =>
    assert.rejects(once(ee2, 'foo', { signal }), { code: 'ERR_INVALID_ARG_TYPE' }),
  ));
  await assert.rejects(once(ee2, 'foo', { signal: already }), { name: 'AbortError' });

  const stopped = new EventTarget();
  const controller = new AbortController();
  controller.signal.addEventListener('abort', (event) => event.stopImmediatePropagation(), { once: true });
  const stoppedPromise = once(stopped, 'foo', { signal: controller.signal });
  process.nextTick(() => controller.abort());
  await assert.rejects(stoppedPromise, { name: 'AbortError' });

  const signalErrors = new EventEmitter();
  const signalController = new AbortController();
  const signalError = new Error('signal-boom');
  process.nextTick(() => signalErrors.emit('error', signalError));
  await assert.rejects(
    once(signalErrors, 'event', { signal: signalController.signal }),
    (error) => error === signalError,
  );

  const eventTarget = new EventTarget();
  const event = new Event('target');
  process.nextTick(() => eventTarget.dispatchEvent(event));
  assert.deepStrictEqual(await once(eventTarget, 'target'), [event]);
  const errorTarget = new EventTarget();
  const errorEvent = new Event('error');
  process.nextTick(() => errorTarget.dispatchEvent(errorEvent));
  assert.deepStrictEqual(await once(errorTarget, 'error'), [errorEvent]);

  const prioritized = new EventEmitter();
  prioritized.addEventListener = () => assert.fail('wrong target API');
  prioritized.removeAllListeners = () => assert.fail('wrong target API');
  process.nextTick(() => prioritized.emit('foo'));
  await once(prioritized, 'foo');

  const afterEvent = new EventEmitter();
  const afterController = new AbortController();
  process.nextTick(() => { afterEvent.emit('foo'); afterController.abort(); });
  await once(afterEvent, 'foo', { signal: afterController.signal });
  assert.strictEqual(afterEvent.listenerCount('foo'), 0);

  const targetController = new AbortController();
  const targetPending = once(new EventTarget(), 'later', { signal: targetController.signal });
  process.nextTick(() => targetController.abort());
  await assert.rejects(targetPending, { name: 'AbortError' });
})();
