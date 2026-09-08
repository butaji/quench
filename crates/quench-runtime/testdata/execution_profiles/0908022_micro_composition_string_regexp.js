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
      var t = 0;
      for (var i = 0; i < s.n; i++)
        t += ("key=" + (i + s.seed)).replace(/\d+/g, "x").length;
      return t;
    };
const check = null;
const state = setup(64, 17, "string_regexp");
assert(typeof operation === "function", "scenario operation is callable");
function run() {
  const value = operation(state);
  if (check) check(value, state, "string_regexp");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"number\",\"320\"]", "exact encoded result");
  return signature;
}
return run();
