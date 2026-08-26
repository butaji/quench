'use strict';

const assert = require('assert');

const result = process.cpuUsage();
assert.strictEqual(typeof result.user, 'number');
assert.strictEqual(typeof result.system, 'number');
assert.doesNotThrow(() => process.cpuUsage(result));
assert.throws(() => process.cpuUsage(1), { code: 'ERR_INVALID_ARG_TYPE' });
assert.throws(() => process.cpuUsage({}), { code: 'ERR_INVALID_ARG_TYPE' });
assert.throws(() => process.cpuUsage({ user: -1, system: 0 }), {
  code: 'ERR_INVALID_ARG_VALUE',
});
