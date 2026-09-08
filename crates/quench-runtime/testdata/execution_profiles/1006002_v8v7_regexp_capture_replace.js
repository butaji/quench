"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("regexp replace assertion failed: " + message);
}
var calls = 0;
var result = "a:1 bb:20".replace(/([a-z]+):(\d+)/g, function (_, name, value) {
  calls++;
  return name.toUpperCase() + String(Number(value) + 1);
});
assert(result === "A2 BB21", "capture replacement result");
assert(calls === 2, "replacement callback count");
return result;
