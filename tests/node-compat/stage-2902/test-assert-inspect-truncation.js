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

instance.throws(() => instance.notStrictEqual(actual, actual), (error) => {
  assert.strictEqual(error.message, `Expected "actual" to be strictly unequal to:\n\n'${actual}'`);
  assert.match(inspect(error), /actual: 'A{9488}\.\.\.'/);
  return true;
});

instance.throws(() => instance.deepEqual(actual, expected), (error) => {
  assert.strictEqual(error.message, `Expected values to be loosely deep-equal:\n\n'${actual}'\n\nshould loosely deep-equal\n\n'${expected}'`);
  assert.match(inspect(error), /expected: 'B{9488}\.\.\.'/);
  return true;
});

const linesA = 'A\n'.repeat(100);
const linesB = 'B\n'.repeat(100);
const truncatedA = 'A\\n'.repeat(10) + '...';
const truncatedB = 'B\\n'.repeat(10) + '...';
instance.throws(() => instance.strictEqual(linesA, linesB), (error) => {
  assert.strictEqual(error.message.split('\n').length, 204);
  assert.strictEqual(error.actual.split('\n').length, 101);
  assert.ok(inspect(error).includes(`actual: '${truncatedA}`));
  assert.ok(inspect(error).includes(`expected: '${truncatedB}`));
  return true;
});
instance.throws(() => instance.notStrictEqual(linesA, linesA), (error) => {
  assert.strictEqual(error.message.split('\n').length, 103);
  assert.ok(inspect(error).includes(`actual: '${truncatedA}`));
  assert.ok(inspect(error).includes(`expected: '${truncatedA}`));
  return true;
});
instance.throws(() => instance.deepEqual(linesA, linesB), (error) => {
  assert.strictEqual(error.message.split('\n').length, 205);
  assert.ok(inspect(error).includes(`expected: '${truncatedB}`));
  return true;
});

const simpleLines = 'fhqwhgads\n'.repeat(15);
assert.throws(() => assert.strictEqual(simpleLines, ''), (error) => {
  assert.ok(inspect(error).includes("actual: 'fhqwhgads\\n' +\n"));
  return true;
});
