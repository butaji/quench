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
    var live = [];
    for (var i = 0; i < n; i++) live.push({ x: i + seed });
    return { n: n, seed: seed, live: live, retained: [] };
  };
const operation = function (s) {
      var a = [],
        t = 0;
      function make(x) {
        var data = [x, x + 1];
        return function () {
          return data[0];
        };
      }
      for (var i = 0; i < s.n; i++) a.push(make(i));
      s.retained = a;
      for (var j = 0; j < a.length; j++) t += a[j]();
      return t;
    };
const check = null;
const state = setup(64, 17, "closure_retention");
assert(typeof operation === "function", "scenario operation is callable");
function verify(value) {
  if (check) check(value, state, "closure_retention");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"number\",\"2016\"]", "exact encoded result: " + signature);
  return signature;
}
return { run: operation, arguments: [state], verify: verify };
