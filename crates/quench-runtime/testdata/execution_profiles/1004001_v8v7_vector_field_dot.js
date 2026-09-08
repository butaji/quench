"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("vector assertion failed: " + message);
}
function Vector(x, y, z) {
  this.x = x;
  this.y = y;
  this.z = z;
}
Vector.prototype.dot = function (other) {
  return this.x * other.x + this.y * other.y + this.z * other.z;
};
var left = new Vector(1, 2, 3);
var right = new Vector(4, 5, 6);
function run() {
  return left.dot(right);
}
function verify(result) {
assert(result === 32, "three-component dot product");
  return result;
}
return { run: run, verify: verify };
