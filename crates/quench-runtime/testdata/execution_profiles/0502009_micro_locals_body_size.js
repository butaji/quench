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
      function f(x) {
        if (x < 0) {
          x += 1;
          x *= 3;
          x -= 7;
          x ^= 3;
          x += 9;
          x *= 5;
          x -= 1;
          x ^= 17;
        }
        return x + 1;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += f(i + s.seed);
      return t;
    };
const check = null;
const state = setup(64, 17, "body_size");
assert(typeof operation === "function", "scenario operation is callable");
function verify(value) {
  if (check) check(value, state, "body_size");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"number\",\"3168\"]", "exact encoded result");
  return signature;
}
return { run: operation, arguments: [state], verify: verify };
