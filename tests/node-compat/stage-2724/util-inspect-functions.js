const assert = require('assert');
const util = require('util');

function abc() {}
assert.strictEqual(util.inspect(abc), '[Function: abc]');
assert.strictEqual(util.inspect(() => 1), '[Function (anonymous)]');
assert.strictEqual(util.inspect(function* () {}), '[GeneratorFunction (anonymous)]');
assert.strictEqual(util.inspect(async function* named() {}), '[AsyncGeneratorFunction: named]');

console.log('util.inspect function values: ok');
