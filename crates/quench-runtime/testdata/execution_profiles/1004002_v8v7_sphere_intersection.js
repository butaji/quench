"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("intersection assertion failed: " + message);
}
function intersect(originZ, directionZ, centerZ, radius) {
  var distance = originZ - centerZ;
  var b = distance * directionZ;
  var c = distance * distance - radius * radius;
  var discriminant = b * b - c;
  if (discriminant <= 0) return null;
  return -b - Math.sqrt(discriminant);
}
function run() {
  return intersect(-5, 1, 0, 1);
}
function verify(result) {
assert(result === 4, "nearest positive intersection");
  return result;
}
return { run: run, verify: verify };
