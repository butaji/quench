"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("higher-order assertion failed: " + message);
}
function square(value) {
  return value * value;
}
function apply(operation, value) {
  return operation(value);
}
var result = 0;
for (var i = 0; i < 8; i++) result += apply(square, i);
assert(result === 140, "known higher-order target result");
return result;
