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
const setup = function (n) {
    var keys = [],
      map = new Map();
    for (var i = 0; i < n; i++) {
      var key = { id: i };
      keys.push(key);
      map.set(key, i);
    }
    return { n: n, keys: keys, map: map };
  };
const operation = function (s) {
      var m = new Map(),
        t = 0;
      for (var i = 0; i < s.n; i++) {
        m.set(i, i);
        t += m.get(i);
        m.delete(i);
      }
      return [t, m.size];
    };
const check = null;
const state = setup(64, 17, "churn");
assert(typeof operation === "function", "scenario operation is callable");
function run() {
  const value = operation(state);
  if (check) check(value, state, "churn");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"array\",[[\"number\",\"2016\"],[\"number\",\"0\"]]]", "exact encoded result");
  return signature;
}
return run();
