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
      var set = new Set(),
        t = 0;
      for (var i = 0; i < s.n; i++) set.add(i);
      for (var j = 0; j < s.n; j++) if (set.has(j)) t++;
      return t;
    };
const check = null;
const state = setup(64, 17, "set");
assert(typeof operation === "function", "scenario operation is callable");
function verify(value) {
  if (check) check(value, state, "set");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"number\",\"64\"]", "exact encoded result");
  return signature;
}
return { run: operation, arguments: [state], verify: verify };
