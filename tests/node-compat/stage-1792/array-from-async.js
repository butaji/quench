"use strict";
const assert = require("assert");

const values = Array.fromAsync(
  (async function* () {
    yield 1;
    yield 2;
  })(),
  (value) => value * 2,
);
values.then((result) => {
  assert.deepStrictEqual(result, [2, 4]);
  console.log("array fromAsync passed");
});
