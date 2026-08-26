"use strict";
const assert = require("assert");

assert.deepStrictEqual([...Iterator.from([1, 2, 3]).drop(1)], [2, 3]);
assert.deepStrictEqual([...Iterator.from([1, 2, 3]).take(2)], [1, 2]);
assert.throws(() => Iterator.from([]).take(), RangeError);

let closed = false;
const iterator = Iterator.from({
  next() {
    return { value: 1, done: false };
  },
  return() {
    closed = true;
    return {};
  },
  [Symbol.iterator]() {
    return this;
  }
});
const taken = iterator.take(0);
assert.deepStrictEqual(taken.next(), { value: undefined, done: true });
assert.strictEqual(closed, true);
