const assert = require('node:assert');
const { setInterval } = require('node:timers/promises');

async function main() {
  const interval = setInterval(1, 'value');
  const iterator = interval[Symbol.asyncIterator]();
  const first = await iterator.next();
  assert.deepStrictEqual(first, { value: 'value', done: false });
  const second = await iterator.next();
  assert.deepStrictEqual(second, { value: 'value', done: false });
  assert.deepStrictEqual(await iterator.return(), { value: undefined, done: true });
  const controller = new AbortController();
  const aborted = setInterval(100, 'never', { signal: controller.signal });
  const pending = aborted[Symbol.asyncIterator]().next();
  controller.abort();
  await assert.rejects(pending, { name: 'AbortError', code: 'ABORT_ERR' });
  console.log('timers/promises interval: ok');
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
