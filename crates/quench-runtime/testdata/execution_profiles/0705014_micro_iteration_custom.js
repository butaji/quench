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
    var a = [];
    for (var i = 0; i < n; i++) a.push(i);
    return { n: n, a: a };
  };
const operation = function (s) {
      var iterable = {};
      iterable[Symbol.iterator] = function () {
        var i = 0;
        return {
          next: function () {
            return { value: i, done: i++ >= s.n };
          }
        };
      };
      var t = 0;
      for (var x of iterable) t += x;
      return t;
    };
const check = function (r, s, v) {
    if ((v === "close" || v === "throw_close") && r[1] !== 1)
      throw new Error("iterator close");
  };
const state = setup(64, 17, "custom");
assert(typeof operation === "function", "scenario operation is callable");
function run() {
  const value = operation(state);
  if (check) check(value, state, "custom");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"number\",\"2016\"]", "exact encoded result");
  return signature;
}
return run();
