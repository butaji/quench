"use strict";
const assert = require("assert");

let iterator;
let returnCalls = 0;
const source = {
  next() {
    return { value: 1, done: false };
  },
  return() {
    returnCalls += 1;
    assert.deepStrictEqual(iterator.return(), { value: undefined, done: true });
    return {};
  },
  [Symbol.iterator]() {
    return this;
  }
};

iterator = Iterator.concat(source);
iterator.next();
assert.deepStrictEqual(iterator.return(), { value: undefined, done: true });
assert.strictEqual(returnCalls, 1);
