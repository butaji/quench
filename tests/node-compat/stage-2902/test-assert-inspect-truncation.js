const assert = require('assert');
const { inspect } = require('util');

const actual = 'A'.repeat(10_000);
const expected = 'B'.repeat(10_000);
const instance = new assert.Assert({ diff: 'full', strict: false });
instance.throws(() => instance.strictEqual(actual, expected), (error) => {
  assert.match(inspect(error), /actual: 'A{9488}\.\.\.'/);
  assert.match(inspect(error), /expected: 'B{9488}\.\.\.'/);
  return true;
});
