"use strict";
const assert = require("assert");
const timers = require("timers");
const { promisify } = require("util");
const setPromiseTimeout = promisify(timers.setTimeout);
const { setInterval } = require("timers/promises");

let count = 0;
async function run() {
  const interval = setInterval(10, "x");
  for await (const value of interval) {
    assert.strictEqual(value, "x");
    count++;
    await setPromiseTimeout(40);
    if (count === 3) break;
  }
}

run();
setTimeout(() => {
  assert.strictEqual(count, 3);
}, 200);
