const assert = require('assert');

const invalidThenable = {
  then(resolve) { resolve(); },
};
const invalidThenableFn = () => {
  const value = () => {};
  value.then = (resolve) => resolve();
  value.catch = () => {};
  return value;
};

assert.rejects(assert.rejects(invalidThenable, {}), {
  code: 'ERR_INVALID_ARG_TYPE',
});
assert.rejects(assert.rejects(invalidThenableFn, {}), {
  code: 'ERR_INVALID_RETURN_VALUE',
});

const rejectingFn = async () => assert.fail();
assert.rejects(rejectingFn, {
  code: 'ERR_ASSERTION',
  name: 'AssertionError',
  message: 'Failed',
});

const err = new Error('foobar');
const validate = () => 'baz';
assert.rejects(
  assert.rejects(Promise.reject(err), validate),
  { code: 'ERR_ASSERTION', operator: 'rejects', actual: err, expected: validate }
);

assert.rejects(
  assert.doesNotReject(() => new Map()),
  { code: 'ERR_INVALID_RETURN_VALUE', name: 'TypeError' }
);

assert.rejects(assert.doesNotReject(Promise.reject(new Error('Failed'))), {
  code: 'ERR_ASSERTION', operator: 'doesNotReject'
});
assert.doesNotReject(async () => {});
assert.doesNotReject(Promise.resolve());
