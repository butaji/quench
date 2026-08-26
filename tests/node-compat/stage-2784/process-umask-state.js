'use strict';
const assert = require('assert');

const original = process.umask();
assert.strictEqual(process.umask('0664'), original);
assert.strictEqual(process.umask(), 0o664);
assert.strictEqual(process.umask(0o022), 0o664);
assert.strictEqual(process.umask(0o10664), 0o022);
assert.strictEqual(process.umask(), 0o664);
assert.throws(() => process.umask({}), { code: 'ERR_INVALID_ARG_TYPE' });
assert.throws(() => process.umask('999'), { code: 'ERR_INVALID_ARG_VALUE' });
