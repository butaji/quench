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
const setup = function (n, seed, v) {
    var a = [];
    for (var i = 0; i < n; i++) {
      if (v !== "holey" || i % 4)
        a[v === "sparse" ? i * 97 : i] = (i + seed) % 31;
    }
    return { n: n, a: a, seed: seed };
  };
const operation = function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.a[i * 97];
      return t;
    };
const check = null;
const state = setup(64, 17, "sparse");
assert(typeof operation === "function", "scenario operation is callable");
function run() {
  return operation(state);
}
function verify(value) {
  if (check) check(value, state, "sparse");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"number\",\"965\"]", "exact encoded result");
  return signature;
}
return { run: run, verify: verify };
