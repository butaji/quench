const assert = require('assert');
const url = require('url');
assert.strictEqual(typeof url.resolveObject, 'function');
assert.strictEqual(url.resolve('', 'foo'), 'foo');
assert.strictEqual(url.resolveObject('', 'foo'), 'foo');
