"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("limb assertion failed: " + message);
}
function multiplyLimbs(source, factor, carry) {
  var output = new Array(source.length);
  for (var i = 0; i < source.length; i++) {
    var value = (source[i] * factor + carry) | 0;
    output[i] = value & 0x3ffffff;
    carry = value >>> 26;
  }
  return [output, carry];
}
var output;
function run() {
  var pair = multiplyLimbs([0x123456, 0x234567, 0x345678, 0x456789], 33, 17);
  output = pair[0];
  var result = pair[1];
  for (var i = 0; i < output.length; i++) result = (result + output[i]) >>> 0;
  return result;
}
function verify(result) {
assert(output.join(",") === "39370535,9171527,46081401,15882410", "exact limbs");
assert(result === 110505875, "limb checksum");
  return result;
}
return { run: run, verify: verify };
