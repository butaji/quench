"use strict";

const assert = require("assert");
const processApi = require("process");

const order = [];
processApi.nextTick(
  (first, second) => {
    order.push("tick");
    assert.strictEqual(first, "first");
    assert.strictEqual(second, 2);
  },
  "first",
  2,
);
order.push("sync");

assert.deepStrictEqual(order, ["sync"]);
queueMicrotask(() => assert.deepStrictEqual(order, ["sync", "tick"]));

console.log("process nextTick arguments passed");
