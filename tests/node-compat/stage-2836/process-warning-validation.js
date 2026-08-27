const assert = require('assert');

assert.throws(() => process.emitWarning(1), { code: 'ERR_INVALID_ARG_TYPE' });
assert.throws(() => process.emitWarning('x', 1), { code: 'ERR_INVALID_ARG_TYPE' });
assert.throws(() => process.emitWarning('x', 'Warning', {}), {
  code: 'ERR_INVALID_ARG_TYPE'
});
process.emitWarning('x');
process.emitWarning('x', 'CustomWarning', 'CODE');
