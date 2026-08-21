const assert = require("assert");

let callbackRefreshCalls = 0;
const callbackRefresh = setTimeout(() => {
  callbackRefreshCalls++;
  if (callbackRefreshCalls === 1) callbackRefresh.refresh();
}, 1);

let firedRefreshCalls = 0;
const firedRefresh = setTimeout(() => {
  firedRefreshCalls++;
  if (firedRefreshCalls === 1) setImmediate(() => firedRefresh.refresh());
}, 1);

let clearedCalls = 0;
const cleared = setTimeout(() => clearedCalls++, 1);
clearTimeout(cleared);
cleared.refresh();

setTimeout(() => {
  assert.strictEqual(callbackRefreshCalls, 2);
  assert.strictEqual(firedRefreshCalls, 2);
  assert.strictEqual(clearedCalls, 0);
}, 20);
