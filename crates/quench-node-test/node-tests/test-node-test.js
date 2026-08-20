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
console.log('node-test: ok');
