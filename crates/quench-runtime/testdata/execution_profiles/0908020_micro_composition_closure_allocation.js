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
    return { n: n, seed: seed };
  };
const operation = function (s) {
      function make(x) {
        return function (y) {
          return x + y;
        };
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += make(i)(s.seed);
      return t;
    };
const check = null;
const state = setup(64, 17, "closure_allocation");
assert(typeof operation === "function", "scenario operation is callable");
function verify(value) {
  if (check) check(value, state, "closure_allocation");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"number\",\"3104\"]", "exact encoded result");
  return signature;
}
return { run: operation, arguments: [state], verify: verify };
