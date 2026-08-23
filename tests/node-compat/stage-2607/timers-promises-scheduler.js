"use strict";

const assert = require("assert");
const { scheduler } = require("node:timers/promises");

(async () => {
  let callbacks = 0;
  await scheduler.wait(0);
  Promise.resolve().then(() => { callbacks += 1; });
  await scheduler.yield();
  assert.strictEqual(callbacks, 1);
  console.log("ok");
})();
