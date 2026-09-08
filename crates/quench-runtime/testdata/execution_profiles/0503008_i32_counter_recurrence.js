"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("counter recurrence assertion failed: " + message);
}
function recurrence(value, steps) {
  while (steps-- > 1) value = (value * 5 + steps + 11) | 0;
  return value;
}
function verify(result) {
  assert(result === 1302, "parameterized recurrence result");
  return result;
}
return { run: recurrence, arguments: [7, 4], verify: verify };
