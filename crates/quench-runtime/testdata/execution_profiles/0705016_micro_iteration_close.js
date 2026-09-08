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
      var closed = 0;
      function* values() {
        try {
          for (var i = 0; i < s.n; i++) yield i;
        } finally {
          closed++;
        }
      }
      var t = 0;
      for (var x of values()) {
        t += x;
        if (x === s.n >> 1) break;
      }
      return [t, closed];
    };
const check = function (r, s, v) {
    if ((v === "close" || v === "throw_close") && r[1] !== 1)
      throw new Error("iterator close");
  };
const state = setup(64, 17, "close");
assert(typeof operation === "function", "scenario operation is callable");
function run() {
  return operation(state);
}
function verify(value) {
  if (check) check(value, state, "close");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"array\",[[\"number\",\"528\"],[\"number\",\"1\"]]]", "exact encoded result");
  return signature;
}
return { run: run, verify: verify };
