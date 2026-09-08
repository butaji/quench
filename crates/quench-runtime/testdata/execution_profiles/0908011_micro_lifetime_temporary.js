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
      for (var i = 0; i < s.n; i++) a.push({ x: i, data: [i, i + 1, i + 2] });
      for (var j = 0; j < a.length; j++) t += a[j].x;
      return t;
    };
const check = null;
const state = setup(64, 17, "temporary");
assert(typeof operation === "function", "scenario operation is callable");
function run() {
  return operation(state);
}
function verify(value) {
  if (check) check(value, state, "temporary");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"number\",\"2016\"]", "exact encoded result");
  return signature;
}
return { run: run, verify: verify };
