// Node compat: assert module semantics.
const assert = require('node:assert');
const strict = require('node:assert/strict');

function expectAssertion(fn, operator) {
  try {
    fn();
  } catch (err) {
    if (err.name !== 'AssertionError') {
      throw new Error('expected AssertionError, got ' + err.name + ': ' + err.message);
    }
    if (operator && err.operator !== operator) {
      throw new Error('expected operator ' + operator + ', got ' + err.operator);
    }
    return err;
  }
  throw new Error('expected AssertionError, nothing thrown');
}

// callable: assert(x) === assert.ok(x)
assert(true);
assert(1);
assert('x');
const errOk = expectAssertion(() => assert(false), 'ok');
if (errOk.message.indexOf('falsy') === -1) throw new Error('ok message: ' + errOk.message);
assert.ok(1, 'custom message');
const errCustom = expectAssertion(() => assert.ok(0, 'boom message'));
if (errCustom.message !== 'boom message') throw new Error('custom: ' + errCustom.message);

// strict namespace / subpath identity
if (strict.strictEqual !== assert.strictEqual) throw new Error('assert/strict mismatch');
if (assert.strict !== assert) throw new Error('assert.strict !== assert');
assert.strict(true);
expectAssertion(() => strict.strictEqual(1, '1'), 'strictEqual');

// strictEqual / notStrictEqual
assert.strictEqual(1, 1);
assert.strictEqual('a', 'a');
assert.strictEqual(undefined, undefined);
assert.strictEqual(null, null);
expectAssertion(() => assert.strictEqual(1, '1'), 'strictEqual');
expectAssertion(() => assert.strictEqual(0, false), 'strictEqual');
assert.notStrictEqual(1, '1');
assert.notStrictEqual({}, {});
expectAssertion(() => assert.notStrictEqual(2, 2), 'notStrictEqual');

// equal / notEqual (abstract ==)
assert.equal(1, '1');
assert.equal(null, undefined);
assert.equal(0, false);
expectAssertion(() => assert.equal(1, 2), 'equal');
assert.notEqual(1, 2);
assert.notEqual('a', 'b');
expectAssertion(() => assert.notEqual(1, '1'), 'notEqual');

// deepStrictEqual / notDeepStrictEqual
assert.deepStrictEqual({ a: 1, b: [1, 2, { c: 'x' }] }, { a: 1, b: [1, 2, { c: 'x' }] });
assert.deepStrictEqual([1, [2, [3]]], [1, [2, [3]]]);
assert.deepStrictEqual({}, {});
assert.deepStrictEqual(NaN, NaN);
expectAssertion(() => assert.deepStrictEqual({ a: 1 }, { a: 1, b: 2 }), 'deepStrictEqual');
expectAssertion(() => assert.deepStrictEqual([1, 2], [1, 3]), 'deepStrictEqual');
expectAssertion(() => assert.deepStrictEqual({ a: 1 }, [1]), 'deepStrictEqual');
expectAssertion(() => assert.deepStrictEqual(1, '1'), 'deepStrictEqual');
assert.notDeepStrictEqual({ a: 1 }, { a: 2 });
expectAssertion(() => assert.notDeepStrictEqual({ a: 1 }, { a: 1 }), 'notDeepStrictEqual');

// cyclic structures
const cycA = { x: 1 };
cycA.self = cycA;
const cycB = { x: 1 };
cycB.self = cycB;
assert.deepStrictEqual(cycA, cycB);

// deep aliases (v1: strict engine)
assert.deepEqual({ a: [1] }, { a: [1] });
assert.notDeepEqual({ a: [1] }, { a: [2] });

// throws
assert.throws(() => { throw new Error('boom'); });
assert.throws(() => { throw new Error('boom'); }, /boom/);
assert.throws(() => { throw new Error('boom'); }, Error);
assert.throws(() => { throw new TypeError('t'); }, TypeError);
expectAssertion(() => assert.throws(() => {}), 'throws');
expectAssertion(() => assert.throws(() => { throw new Error('boom'); }, /nope/), 'throws');
expectAssertion(() => assert.throws(() => { throw new Error('x'); }, RangeError), 'throws');
assert.throws(() => { throw new Error('boom'); }, { message: 'boom' });
expectAssertion(() => assert.throws(() => { throw new Error('boom'); }, { message: 'other' }), 'throws');
assert.throws(() => { throw new Error('boom'); }, (err) => err.message === 'boom');

// doesNotThrow
assert.doesNotThrow(() => 1);
expectAssertion(() => assert.doesNotThrow(() => { throw new Error('x'); }), 'doesNotThrow');

// fail / ifError
const errFail = expectAssertion(() => assert.fail('dead'), 'fail');
if (errFail.message !== 'dead') throw new Error('fail message');
expectAssertion(() => assert.fail(), 'fail');
assert.ifError(null);
assert.ifError(undefined);
expectAssertion(() => assert.ifError(new Error('e')), 'ifError');

// match / doesNotMatch
assert.match('hello world', /world/);
assert.doesNotMatch('hello', /xyz/);
expectAssertion(() => assert.match('hello', /xyz/), 'match');
expectAssertion(() => assert.doesNotMatch('hello', /ell/), 'doesNotMatch');

// error shape: actual/expected/operator props readable
const errSe = expectAssertion(() => assert.strictEqual(1, 2), 'strictEqual');
if (errSe.actual !== 1 || errSe.expected !== 2) throw new Error('actual/expected props');
if (typeof assert.AssertionError !== 'object' && typeof assert.AssertionError !== 'function') {
  throw new Error('AssertionError export missing');
}


assert.partialDeepStrictEqual({ a: 1 }, { a: 1 });

console.log('assert: ok');
