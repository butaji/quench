"use strict";
const assert = require("assert");

let observed;
const source = {
  next() {
    return { value: 1, done: true };
  },
  [Symbol.iterator]() {
    return this;
  }
};
const proxy = new Proxy(source, {
  get(target, key, receiver) {
    if (typeof key === "symbol") observed = String(key);
    return Reflect.get(target, key, receiver);
  }
});

Iterator.from(proxy);
assert.strictEqual(observed, "Symbol(Symbol.iterator)");
