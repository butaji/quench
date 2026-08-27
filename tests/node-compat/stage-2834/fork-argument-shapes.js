const assert = require('assert');
const { fork } = require('child_process');

assert.strictEqual(typeof fork, 'function');
assert.doesNotThrow(() => fork('empty.js', null, null));
assert.throws(() => fork('empty.js', 'args'), { code: 'ERR_INVALID_ARG_TYPE' });
assert.throws(() => fork('empty.js', [], 'options'), { code: 'ERR_INVALID_ARG_TYPE' });
assert.throws(() => fork('empty.js', [], []), { code: 'ERR_INVALID_ARG_TYPE' });
