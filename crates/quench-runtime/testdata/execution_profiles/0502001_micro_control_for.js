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
      for (var i = 0; i < s.n; i++) sum += s.a[i];
      return sum;
    };
const check = null;
const state = setup(64, 17, "for");
assert(typeof operation === "function", "scenario operation is callable");
function run() {
  return operation(state);
}
function verify(value) {
  if (check) check(value, state, "for");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"number\",\"223\"]", "exact encoded result");
  return signature;
}
return { run: run, verify: verify };
