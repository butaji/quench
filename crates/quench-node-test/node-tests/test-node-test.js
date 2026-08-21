const test = require('node:test');
const assert = require('assert');
let before = 0, after = 0;
test('passing', () => assert.strictEqual(1, 1));
test('subtests', (t) => { t.test('child', () => assert.strictEqual(2, 2)); });
test('skipped', { skip: true }, () => { throw new Error('must not run'); });
test.describe('hooks', () => {
  test.beforeEach(() => { before++; });
  test.afterEach(() => { after++; });
  test('hooked', () => assert.strictEqual(before, 1));
});
test('async', async () => { await Promise.resolve(); assert.strictEqual(3, 3); });
test.todo('todo child', () => { throw new Error('must not run'); });
test.only('selective', () => assert.strictEqual(4, 4));
test.run().then((summary) => {
  if (summary.pass === 0 || summary.skip === 0 || summary.fail < 0) throw new Error('summary');
  console.log('node-test: ok');
});
console.log('node-test: ok');
