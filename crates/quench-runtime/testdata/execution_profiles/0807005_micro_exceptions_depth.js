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
      function f(d, x) {
        if (!d) throw x;
        return f(d - 1, x);
      }
      var t = 0;
      for (var i = 0; i < s.n; i++)
        try {
          f(16, i);
        } catch (e) {
          t += e;
        }
      return t;
    };
const check = function (r, s, v) {
    if (v === "finally" && r[1] !== s.n) throw new Error("finally effects");
  };
const state = setup(64, 17, "depth");
assert(typeof operation === "function", "scenario operation is callable");
function verify(value) {
  if (check) check(value, state, "depth");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"number\",\"2016\"]", "exact encoded result: " + signature);
  return signature;
}
return { run: operation, arguments: [state], verify: verify };
