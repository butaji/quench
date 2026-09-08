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
    var a = [];
    for (var i = 0; i < n; i++) a.push({ x: i + seed });
    return {
      n: n,
      a: a,
      text: JSON.stringify(a),
      date: new Date(Date.UTC(2020, 0, 2)),
      seed: seed
    };
  };
const operation = function (s) {
      var a = JSON.parse(s.text);
      return [a.length, a[a.length - 1].x];
    };
const check = function (r, s, v) {
    if ((v === "json_reviver" || v === "json_replacer") && r[1] !== 2 * s.n + 1)
      throw new Error("JSON callback effects");
  };
const state = setup(64, 17, "json_parse");
assert(typeof operation === "function", "scenario operation is callable");
function verify(value) {
  if (check) check(value, state, "json_parse");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"array\",[[\"number\",\"64\"],[\"number\",\"80\"]]]", "exact encoded result");
  return signature;
}
return { run: operation, arguments: [state], verify: verify };
