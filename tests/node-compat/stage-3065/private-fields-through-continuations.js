"use strict";

const assert = require("assert");

class Counter {
  #value = 41;

  value() {
    return this.#value + 1;
  }
}

assert.strictEqual(new Counter().value(), 42);
