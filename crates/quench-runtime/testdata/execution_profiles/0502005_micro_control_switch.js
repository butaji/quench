"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("execution profile assertion failed: " + message);
}
function encode(value) {
  if (value === undefined) return ["undefined"];
  if (typeof value === "number") return ["number", Number.isNaN(value) ? "NaN" : Object.is(value, -0) ? "-0" : String(value)];
  if (typeof value === "bigint") return ["bigint", String(value)];
  if (value === null || typeof value !== "object") return [typeof value, value];
  if (Array.isArray(value)) return ["array", value.map(encode)];
  return ["object", Object.keys(value).map(function (key) { return [key, encode(value[key])]; })];
}
const setup = function (n, seed) {
    var a = [];
    for (var i = 0; i < n; i++) a.push(((i * 1103515245 + seed) >>> 8) & 7);
    return { n: n, a: a };
  };
const operation = function (s) {
      var sum = 0;
      for (var i = 0; i < s.n; i++) {
        switch (s.a[i]) {
          case 0:
            sum += 3;
            break;
          case 1:
            sum += 7;
            break;
          case 2:
            sum -= 1;
            break;
          default:
            sum += 2;
        }
      }
      return sum;
    };
const check = null;
const state = setup(64, 17, "switch");
assert(typeof operation === "function", "scenario operation is callable");
function run() {
  const value = operation(state);
  if (check) check(value, state, "switch");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"number\",\"148\"]", "exact encoded result");
  return signature;
}
return run();
