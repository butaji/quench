const assert = require('assert');
const util = require('util');

assert.strictEqual(util.format('%s', () => 5), '() => 5');
assert.strictEqual(util.format('%s', { a: [1, 2, 3] }), '{ a: [Array] }');

console.log('util-format function values: ok');
