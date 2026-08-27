const assert = require('assert');

assert.throws(() => process.emitWarning({}), { code: 'ERR_INVALID_ARG_TYPE' });
assert.throws(() => process.emitWarning([]), { code: 'ERR_INVALID_ARG_TYPE' });

const warning = new Error('payload');
warning.name = 'CustomWarning';
process.emitWarning(warning);

const named = { name: 'NamedWarning', message: 'payload' };
process.emitWarning(named);
