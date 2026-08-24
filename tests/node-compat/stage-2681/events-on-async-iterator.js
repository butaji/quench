const { on, EventEmitter } = require('events');

(async () => {
  const emitter = new EventEmitter();
  const iterable = on(emitter, 'value');
  process.nextTick(() => {
    emitter.emit('value', 42);
    iterable.return();
  });
  const first = await iterable.next();
  const done = await iterable.next();
  if (first.value[0] !== 42 || first.done || !done.done) process.exit(1);
  if (emitter.listenerCount('value') !== 0 || emitter.listenerCount('error') !== 0) process.exit(1);

  const invalid = [1, null, false, 'bad'];
  for (const signal of invalid) {
    try { on(emitter, 'value', { signal }); process.exit(1); } catch (error) {
      if (error.code !== 'ERR_INVALID_ARG_TYPE') process.exit(1);
    }
  }
})();
