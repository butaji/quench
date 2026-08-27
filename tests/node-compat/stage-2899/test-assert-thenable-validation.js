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
