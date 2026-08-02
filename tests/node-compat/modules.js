const assert = require('node:assert');
const path = require('path');
const common = require('../common');
assert.strictEqual(path.basename('/tmp/example.txt'), 'example.txt');
assert.strictEqual(path.dirname('/tmp/example.txt'), '/tmp');
assert.strictEqual(path.extname('example.txt'), '.txt');
assert.strictEqual(typeof common.mustCall(() => {}), 'function');
