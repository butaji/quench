"use strict";

const assert = require("assert");
const { setInterval } = require("timers/promises");

const iterator = setInterval(1, "value");
iterator
  .next()
  .then((result) => {
    assert.deepStrictEqual(result, { value: "value", done: false });
    return iterator.return();
  })
  .then((result) => assert.deepStrictEqual(result, { value: undefined, done: true }));
