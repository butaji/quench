"use strict";
const assert = require("assert");

const expected = { value: 5, done: true };
let returnCalls = 0;
const source = {
  next() {
    return { value: 1, done: false };
  },
  return() {
    returnCalls += 1;
    return expected;
  },
  [Symbol.iterator]() {
    return this;
  }
};

const wrapper = Iterator.from(source);
assert.strictEqual(wrapper.return(), expected);
assert.strictEqual(returnCalls, 1);
