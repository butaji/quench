"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("constraint assertion failed: " + message);
}
function Variable(value) {
  this.value = value;
}
function ScaleConstraint(input, output, scale, offset) {
  this.input = input;
  this.output = output;
  this.scale = scale;
  this.offset = offset;
}
ScaleConstraint.prototype.execute = function () {
  this.output.value = this.input.value * this.scale + this.offset;
};
var input = new Variable(0);
var output = new Variable(0);
var constraint = new ScaleConstraint(input, output, 3, 2);
function run() {
  var result = 0;
  for (var i = 0; i < 8; i++) {
    input.value = i;
    constraint.execute();
    result += output.value;
  }
  return result;
}
function verify(result) {
assert(result === 100, "propagated values");
  return result;
}
return { run: run, verify: verify };
