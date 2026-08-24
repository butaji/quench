const assert = require('assert');
const util = require('util');

function abc() {}
assert.strictEqual(util.inspect(abc), '[Function: abc]');
assert.strictEqual(util.inspect(() => 1), '[Function (anonymous)]');

console.log('util.inspect function values: ok');
