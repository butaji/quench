"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("one-trip recurrence assertion failed: " + message);
}
function run(value, steps) {
  while (steps-- > 2) value = (value * 7 + steps + 2) | 0;
  return value;
}
function verify(result) {
  assert(result === -24, "one iteration uses the decremented counter");
  return result;
}
return { run: run, arguments: [-4, 3], verify: verify };
