"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("shift assertion failed: " + message);
}
var value = 0x12345678 | 0;
for (var i = 0; i < 8; i++) {
  value = ((value << 5) ^ (value >>> 3) ^ i) | 0;
}
var result = value >>> 0;
assert(result === 543300779, "ToInt32 and unsigned shift result");
return result;
