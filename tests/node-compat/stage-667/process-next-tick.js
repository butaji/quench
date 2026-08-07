"use strict";

const assert = require("assert");
const processApi = require("process");

let received;
processApi.nextTick(
  (first, second) => {
    received = [first, second];
  },
  "first",
  2,
);
queueMicrotask(() => assert.deepStrictEqual(received, ["first", 2]));

console.log("process nextTick passed");
