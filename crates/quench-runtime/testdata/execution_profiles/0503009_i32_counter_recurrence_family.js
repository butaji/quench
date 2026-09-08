"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("counter recurrence family assertion failed: " + message);
}
function positive(value, steps) {
  while (steps-- > 0) value = (value * 3 + steps + 7) | 0;
  return value;
}
function alternate(value, steps) {
  while (steps-- > 2) value = (value * 7 + steps + 2) | 0;
  return value;
}
function empty(value, steps) {
  while (steps-- > 10) value = (value * 9 + steps + 4) | 0;
  return value;
}
function run() {
  return positive(2, 3) + alternate(-4, 5) + empty(9, 2);
}
function verify(result) {
  assert(result === -864, "constant, signed-input, and trip-count variations");
  return result;
}
return { run: run, arguments: [], verify: verify };
