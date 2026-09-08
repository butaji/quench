"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("neighbor assertion failed: " + message);
}
var values = [0, 1, 2, 3, 4, 5, 6];
var source = [0, 2, 4, 6, 8, 10, 12];
function run() {
  for (var i = 1; i <= 5; i++) {
    values[i] = (source[i] + 0.5 * (values[i - 1] + values[i + 1])) / 2;
  }
  var result = 0;
  for (var j = 1; j <= 5; j++) result += values[j];
  return result;
}
function verify(result) {
assert(result === 23.962890625, "ordered loop-carried update");
  return result;
}
return { run: run, verify: verify };
