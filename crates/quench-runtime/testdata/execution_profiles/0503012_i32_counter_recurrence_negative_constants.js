"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("negative recurrence assertion failed: " + message);
}
function run(value, steps) {
  while (steps-- > -1) value = (value * -2 + steps + -5) | 0;
  return value;
}
function verify(result) {
  assert(result === 268, "negative constants retain Number evaluation order");
  return result;
}
return { run: run, arguments: [4, 5], verify: verify };
