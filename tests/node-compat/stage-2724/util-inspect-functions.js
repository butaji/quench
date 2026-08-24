const assert = require('assert');
const util = require('util');

function abc() {}
assert.strictEqual(util.inspect(abc), '[Function: abc]');
assert.strictEqual(util.inspect(() => 1), '[Function (anonymous)]');
assert.strictEqual(util.inspect(function* () {}), '[GeneratorFunction (anonymous)]');
assert.strictEqual(util.inspect(async function* named() {}), '[AsyncGeneratorFunction: named]');
assert.strictEqual(util.inspect(/foo(bar\n)?/gi), '/foo(bar\\n)?/gi');
assert.strictEqual(util.inspect(new Date('2010-02-14T11:48:40.000Z')), '2010-02-14T11:48:40.000Z');
assert.strictEqual(util.inspect('\n\x01'), "'\\n\\x01'");

console.log('util.inspect function values: ok');
