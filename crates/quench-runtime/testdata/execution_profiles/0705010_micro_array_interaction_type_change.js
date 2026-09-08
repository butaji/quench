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
      var a = [],
        t = 0;
      for (var i = 0; i < s.n; i++) a[i] = i;
      a[s.n >> 1] = "7";
      for (var j = 0; j < s.n; j++) t += +a[j];
      return t;
    };
const check = function (r, s, v) {
    if (v === "inherited_index" && r[1] !== s.n)
      throw new Error("indexed getter count");
  };
const state = setup(64, 17, "type_change");
assert(typeof operation === "function", "scenario operation is callable");
function run() {
  return operation(state);
}
function verify(value) {
  if (check) check(value, state, "type_change");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"number\",\"1991\"]", "exact encoded result");
  return signature;
}
return { run: run, verify: verify };
