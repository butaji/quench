const assert = require('node:assert');
const { AsyncLocalStorage } = require('node:async_hooks');

const als = new AsyncLocalStorage();
async function main() {
  await 0;
  als.enterWith('after await');
  await Promise.resolve().then(() => als.enterWith('inside then'));
  assert.strictEqual(als.getStore(), undefined);
  als.enterWith('before await');
  await 0;
  assert.strictEqual(als.getStore(), 'before await');
  console.log('async local context: ok');
}
main().catch((error) => { console.error(error); process.exitCode = 1; });
