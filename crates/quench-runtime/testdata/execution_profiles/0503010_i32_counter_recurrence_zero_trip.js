"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("zero-trip recurrence assertion failed: " + message);
}
function run(value, steps) {
  while (steps-- > 10) value = (value * 9 + steps + 4) | 0;
  return value;
}
function verify(result) {
  assert(result === 9, "zero body iterations preserve the initial value");
  return result;
}
return { run: run, arguments: [9, 2], verify: verify };
