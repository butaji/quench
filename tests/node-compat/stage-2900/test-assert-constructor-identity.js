const assert = require('assert');
let error;
try { assert(false); } catch (value) { error = value; }
assert.strictEqual(error.constructor, assert.AssertionError, 'constructor identity');
assert(error instanceof assert.AssertionError, 'instanceof');
assert(assert.AssertionError.prototype instanceof Error, 'prototype instanceof Error');
assert.throws(() => { throw new assert.AssertionError({}); }, assert.AssertionError);
function thrower(errorConstructor) { throw new errorConstructor({}); }
assert.throws(() => thrower(assert.AssertionError), assert.AssertionError);
