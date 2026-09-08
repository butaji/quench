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
    return { n: n };
  };
const operation = function (s) {
      var o = {},
        t = 0;
      for (var i = 0; i < s.n; i++) {
        Object.defineProperty(o, "x", {
          value: i,
          writable: true,
          configurable: true
        });
        t += o.x;
      }
      return t;
    };
const check = function (r, s, v) {
    if (v === "accessor_transition" && r[1] !== s.n)
      throw new Error("getter count");
  };
const state = setup(64, 17, "descriptor");
assert(typeof operation === "function", "scenario operation is callable");
function verify(value) {
  if (check) check(value, state, "descriptor");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"number\",\"2016\"]", "exact encoded result");
  return signature;
}
return { run: operation, arguments: [state], verify: verify };
