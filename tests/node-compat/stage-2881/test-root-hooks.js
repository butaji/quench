const assert = require('assert');
const { before, after, test, getTestContext } = require('node:test');

let setup = 0;
let teardown = 0;
before(() => { setup++; });
after(() => { teardown++; });
test('root hooks surround the test', () => {
  assert.strictEqual(setup, 1);
  assert.strictEqual(getTestContext().name, 'root hooks surround the test');
});
process.on('exit', () => {
  assert.strictEqual(teardown, 1);
});
