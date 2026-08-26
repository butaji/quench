"use strict";
const assert = require("assert");

assert.strictEqual(
  Iterator.from([1, 2, 3]).reduce((memo, value, index) => memo + value + index),
  9
);
assert.strictEqual(
  Iterator.from([1, 2, 3]).find((value) => value === 2),
  2
);
let total = 0;
assert.strictEqual(
  Iterator.from([1, 2, 3]).forEach((value) => {
    total += value;
  }),
  undefined
);
assert.strictEqual(total, 6);

let closed = false;
const closable = {
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
};
assert.throws(() => Iterator.from(closable).find(), TypeError);
assert.strictEqual(closed, true);
