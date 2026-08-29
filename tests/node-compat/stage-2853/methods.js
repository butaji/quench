'use strict';
const assert = require('assert');
const { test } = require('node:test');

test('mock method preserves receiver and restores', (t) => {
  const object = { value: 5, add(value) { return this.value + value; } };
  const original = object.add;
  const mock = t.mock.method(object, 'add');
  assert.strictEqual(object.add(3), 8);
  assert.strictEqual(mock.mock.calls[0].this, object);
  assert.strictEqual(mock.mock.calls[0].result, 8);
  assert.strictEqual(mock.mock.restore(), undefined);
  assert.strictEqual(object.add, original);
});

test('mock accessor getter and setter', (t) => {
  const object = { value: 5, get answer() { return this.value; }, set answer(value) { this.value = value; } };
  const getter = t.mock.method(object, 'answer', { getter: true });
  assert.strictEqual(object.answer, 5);
  assert.strictEqual(getter.mock.calls[0].this, object);
  getter.mock.restore();
  const setter = t.mock.method(object, 'answer', { setter: true });
  object.answer = 8;
  assert.strictEqual(object.value, 8);
  assert.strictEqual(setter.mock.calls[0].arguments[0], 8);
  setter.mock.restore();
  assert.strictEqual(object.answer, 8);
});
