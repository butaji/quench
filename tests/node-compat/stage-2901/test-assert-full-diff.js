const assert = require('assert');
const longStringOfAs = 'A'.repeat(10_000);
const longStringOfBs = 'B'.repeat(10_000);
const instance = new assert.Assert({ diff: 'full', strict: false });

instance.throws(() => instance.strictEqual(longStringOfAs, longStringOfBs), (error) => {
  assert.strictEqual(error.diff, 'full');
  assert.strictEqual(error.actual, longStringOfAs);
  assert.strictEqual(error.expected, longStringOfBs);
  assert.strictEqual(
    error.message,
    `Expected values to be strictly equal:\n+ actual - expected\n\n+ '${longStringOfAs}'\n- '${longStringOfBs}'\n`
  );
  return true;
});

const linesA = 'A\n'.repeat(100);
const linesB = 'B\n'.repeat(100);
instance.throws(() => instance.strictEqual(linesA, linesB), (error) => {
  assert.strictEqual(error.message.split('\n').length, 204);
  assert.strictEqual(error.actual.split('\n').length, 101);
  return true;
});
