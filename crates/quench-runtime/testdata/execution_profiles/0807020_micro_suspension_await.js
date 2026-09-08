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
const operation = async function (s) {
      var t = s.seed;
      for (var i = 0; i < s.n; i++) t = await Promise.resolve(t + 1);
      return t;
    };
const check = null;
const state = setup(64, 17, "await");
assert(typeof operation === "function", "scenario operation is callable");
async function run() {
  const value = await operation(state);
  if (check) check(value, state, "await");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"number\",\"81\"]", "exact encoded result");
  return signature;
}
return run();
