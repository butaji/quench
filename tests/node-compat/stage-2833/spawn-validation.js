const assert = require('assert');
const { spawn } = require('child_process');

assert.throws(() => spawn(), { code: 'ERR_INVALID_ARG_TYPE' });
assert.throws(() => spawn(''), { code: 'ERR_INVALID_ARG_VALUE' });
assert.throws(() => spawn('echo', 'args'), { code: 'ERR_INVALID_ARG_TYPE' });
assert.throws(() => spawn('echo', [], null), { code: 'ERR_INVALID_ARG_TYPE' });
assert.throws(() => spawn('echo', [], { uid: 2 ** 63 }), {
  code: 'ERR_OUT_OF_RANGE'
});
spawn('echo', null, {});
