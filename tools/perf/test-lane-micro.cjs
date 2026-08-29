#!/usr/bin/env node
"use strict";

const assert = require("node:assert/strict");
const { failuresFor, measure } = require("./assert-lane-micro.cjs");

const snapshot = {
  lanes: { l0: { value_decode: 1 }, l2: { handlers: 99 }, l3: { handlers: 0 } },
};
const ratio = {
  numerator: "lanes.l0.value_decode",
  denominator: { sum: ["lanes.l2.handlers", "lanes.l3.handlers"] },
  min_traffic: 99,
  max: 0.02,
};
assert.deepEqual(measure(snapshot, ratio), { numerator: 1, denominator: 99, value: 1 / 99 });
assert.deepEqual(failuresFor(snapshot, { metrics: { decode: ratio } }), []);

const insufficient = { ...ratio, min_traffic: 100 };
assert.match(failuresFor(snapshot, { metrics: { decode: insufficient } })[0].reason, /traffic/);
const strict = { ...ratio, max: 0.001 };
assert.equal(failuresFor(snapshot, { metrics: { decode: strict } })[0].name, "decode");
console.log("lane micro assertion tests: ok");
