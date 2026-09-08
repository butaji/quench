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
    return { n: n, seed: seed, retained: [] };
  };
const operation = function (s) {
      var a = [];
      function make(x) {
        return function () {
          return x;
        };
      }
      for (var i = 0; i < s.n; i++) a.push(make(i + s.seed));
      s.retained = a;
      var t = 0;
      for (var j = 0; j < a.length; j++) t += a[j]();
      return t;
    };
const check = null;
const state = setup(64, 17, "escaping");
assert(typeof operation === "function", "scenario operation is callable");
function run() {
  return operation(state);
}
function verify(value) {
  if (check) check(value, state, "escaping");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"number\",\"3104\"]", "exact encoded result");
  return signature;
}
return { run: run, verify: verify };
