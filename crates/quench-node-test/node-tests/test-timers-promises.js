const assert = require('node:assert');
const { setTimeout: sleep, setImmediate: immediate } = require('node:timers/promises');

async function main() {
  assert.strictEqual(await sleep(0, 'timeout-value'), 'timeout-value');
  assert.strictEqual(await immediate('immediate-value'), 'immediate-value');
  const controller = new AbortController();
  const pending = sleep(100, 'late', { signal: controller.signal });
  controller.abort();
  await assert.rejects(pending, { name: 'AbortError', code: 'ABORT_ERR' });
  console.log('timers/promises: ok');
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
