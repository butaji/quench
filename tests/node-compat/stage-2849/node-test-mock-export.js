'use strict';

const assert = require('assert');
require('../../node/test/common');
const nodeTest = require('node:test');

assert.strictEqual(typeof nodeTest, 'function');
assert.strictEqual(typeof nodeTest.mock, 'object');
assert.strictEqual(typeof nodeTest.mock.fn, 'function');

nodeTest('context exposes mock', (t) => {
  assert.strictEqual(typeof t.mock, 'object');
  assert.strictEqual(typeof t.mock.fn, 'function');
  const direct = () => 7;
  assert.strictEqual(direct(), 7);
  const wrapped = t.mock.fn(direct);
  assert.strictEqual(typeof wrapped, 'function');
  assert.strictEqual(wrapped(), 7);
  const add = t.mock.fn((a, b) => a + b);
  assert.strictEqual(add(3, 4), 7);
  assert.strictEqual(add.mock.calls.length, 1);
  assert.deepStrictEqual(add.mock.calls[0].arguments, [3, 4]);
  assert.strictEqual(add.mock.calls[0].result, 7);
  assert.strictEqual(add.mock.calls[0].error, undefined);
  assert.strictEqual(add.mock.calls[0].this, undefined);
  const body = t.mock.fn(function (a, b) {
    return a + b;
  });
  assert.strictEqual(body(3, 4), 7);
});

const { mock, test } = require('node:test');
test('destructured mock', (t) => {
  const sum = t.mock.fn((arg1, arg2) => {
    return arg1 + arg2;
  });
  assert.strictEqual(sum.mock.calls.length, 0);
  assert.strictEqual(sum(3, 4), 7);
  assert.strictEqual(sum.call(1000, 9, 1), 10);
  assert.strictEqual(sum.mock.calls.length, 2);
  assert.strictEqual(sum.mock.calls[1].this, 1000);
  assert.strictEqual(sum.mock.calls[1].result, 10);
});

test('constructor mock', (t) => {
  class Parent { constructor(c) { this.c = c; } }
  const C = t.mock.fn(Parent);
  const value = new C(42);
  assert(value instanceof Parent);
  assert.strictEqual(value.c, 42);
  assert.strictEqual(C.mock.calls.length, 1);
  assert.deepStrictEqual(C.mock.calls[0].arguments, [42]);
  assert.strictEqual(C.mock.calls[0].result, value);
  assert.strictEqual(C.mock.calls[0].target, Parent);
  assert.strictEqual(C.mock.calls[0].this, value);
});

test('no-op mock function', (t) => {
  const fn = t.mock.fn();
  assert.strictEqual(fn.mock.calls.length, 0);
  assert.strictEqual(fn(3, 4), undefined);
  assert.strictEqual(fn.mock.calls.length, 1);
  assert.deepStrictEqual(fn.mock.calls[0].arguments, [3, 4]);
  assert.strictEqual(fn.mock.calls[0].result, undefined);
});

test('mock implementation override', (t) => {
  const sum = (a, b) => a + b;
  const difference = (a, b) => a - b;
  const product = (a, b) => a * b;
  const fn1 = t.mock.fn(sum, difference);
  const fn2 = t.mock.fn(sum, product);
  assert.strictEqual(fn1(5, 3), 2);
  assert.strictEqual(fn2(5, 3), 15);
  assert.strictEqual(fn2(4, 2), 8);
});
