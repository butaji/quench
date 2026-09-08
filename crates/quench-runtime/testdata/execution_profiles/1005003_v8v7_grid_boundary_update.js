"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("boundary assertion failed: " + message);
}
var width = 2;
var height = 2;
var rowSize = 4;
var values = new Array(16).fill(0);
values[5] = 1;
values[6] = 2;
values[9] = 3;
values[10] = 4;
for (var i = 1; i <= width; i++) {
  values[i] = values[i + rowSize];
  values[i + (height + 1) * rowSize] = values[i + height * rowSize];
}
for (var j = 1; j <= height; j++) {
  values[j * rowSize] = values[1 + j * rowSize];
  values[width + 1 + j * rowSize] = values[width + j * rowSize];
}
var result = values.reduce(function (sum, value) { return sum + value; }, 0);
assert(result === 30, "boundary copy result");
return result;
