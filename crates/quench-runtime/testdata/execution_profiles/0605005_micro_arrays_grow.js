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
      var a = [];
      for (var i = 0; i < s.n; i++) a.push(i + s.seed);
      return a[a.length - 1];
    };
const check = null;
const state = setup(64, 17, "grow");
assert(typeof operation === "function", "scenario operation is callable");
function verify(value) {
  if (check) check(value, state, "grow");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"number\",\"80\"]", "exact encoded result");
  return signature;
}
return { run: operation, arguments: [state], verify: verify };
