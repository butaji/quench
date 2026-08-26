'use strict';
const assert = require('assert');

for (const name of ['getuid', 'getgid', 'geteuid', 'getegid']) {
  assert.strictEqual(typeof process[name], 'function');
  assert.strictEqual(typeof process[name](), 'number');
}
for (const name of ['setuid', 'setgid', 'seteuid', 'setegid']) {
  assert.throws(() => process[name]({}), { code: 'ERR_INVALID_ARG_TYPE' });
  assert.throws(() => process[name]('missing-user'), {
    code: 'ERR_UNKNOWN_CREDENTIAL',
  });
}
