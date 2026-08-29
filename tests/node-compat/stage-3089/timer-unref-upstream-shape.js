"use strict";

const assert = require("assert");
const common = require("../../node/test/common");
let unrefInterval = false;
let unrefTimer = false;
let checks = 0;
const timer = setTimeout(() => {}, 10);
assert.strictEqual(timer.hasRef(), true);
timer.unref().ref().unref();
assert.strictEqual(timer.hasRef(), false);
setInterval(() => {}, 10)
  .unref()
  .ref()
  .unref();
setInterval(common.mustNotCall("long interval"), 10000).unref();
setTimeout(common.mustNotCall("long timeout"), 10000).unref();
const interval = setInterval(
  common.mustCall(() => {
    unrefInterval = true;
    clearInterval(interval);
  }),
  20
);
interval.unref();
setTimeout(
  common.mustCall(() => {
    unrefTimer = true;
  }),
  20
).unref();
const checker = setInterval(() => {
  if (checks > 5 || (unrefInterval && unrefTimer)) clearInterval(checker);
  checks += 1;
}, 20);
const callbackTimer = setInterval(() => callbackTimer.unref(), 100);
const t = setInterval(() => {}, 1);
process.nextTick(t.unref.bind({}));
process.nextTick(t.unref.bind(t));
setTimeout(() => {
  assert.strictEqual(unrefInterval, true);
  assert.strictEqual(unrefTimer, true);
  assert.strictEqual(checker.hasRef(), false);
  clearInterval(callbackTimer);
}, 100);
