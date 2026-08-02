const assert = require('assert');
const tmpdir = require('../common/tmpdir');
assert.strictEqual(require('path').basename(tmpdir.resolve('foo.')), 'foo.');
