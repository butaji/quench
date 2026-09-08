"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("collection assertion failed: " + message);
}
function removeEven(values) {
  var write = 0;
  for (var read = 0; read < values.length; read++) {
    var value = values[read];
    if ((value & 1) !== 0) values[write++] = value;
  }
  values.length = write;
  return values;
}
var values;
function run() {
  values = removeEven([0, 1, 2, 3, 4, 5, 6, 7]);
  return values[0] + values[1] + values[2] + values[3];
}
function verify(result) {
assert(values.length === 4, "compacted length");
assert(result === 16, "stable retained values");
  return result;
}
return { run: run, verify: verify };
