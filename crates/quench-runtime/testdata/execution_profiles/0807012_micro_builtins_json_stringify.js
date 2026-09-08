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
      return JSON.stringify(s.a);
    };
const check = function (r, s, v) {
    if ((v === "json_reviver" || v === "json_replacer") && r[1] !== 2 * s.n + 1)
      throw new Error("JSON callback effects");
  };
const state = setup(64, 17, "json_stringify");
assert(typeof operation === "function", "scenario operation is callable");
function run() {
  const value = operation(state);
  if (check) check(value, state, "json_stringify");
  const signature = JSON.stringify(encode(value));
  assert(signature === "[\"string\",\"[{\\\"x\\\":17},{\\\"x\\\":18},{\\\"x\\\":19},{\\\"x\\\":20},{\\\"x\\\":21},{\\\"x\\\":22},{\\\"x\\\":23},{\\\"x\\\":24},{\\\"x\\\":25},{\\\"x\\\":26},{\\\"x\\\":27},{\\\"x\\\":28},{\\\"x\\\":29},{\\\"x\\\":30},{\\\"x\\\":31},{\\\"x\\\":32},{\\\"x\\\":33},{\\\"x\\\":34},{\\\"x\\\":35},{\\\"x\\\":36},{\\\"x\\\":37},{\\\"x\\\":38},{\\\"x\\\":39},{\\\"x\\\":40},{\\\"x\\\":41},{\\\"x\\\":42},{\\\"x\\\":43},{\\\"x\\\":44},{\\\"x\\\":45},{\\\"x\\\":46},{\\\"x\\\":47},{\\\"x\\\":48},{\\\"x\\\":49},{\\\"x\\\":50},{\\\"x\\\":51},{\\\"x\\\":52},{\\\"x\\\":53},{\\\"x\\\":54},{\\\"x\\\":55},{\\\"x\\\":56},{\\\"x\\\":57},{\\\"x\\\":58},{\\\"x\\\":59},{\\\"x\\\":60},{\\\"x\\\":61},{\\\"x\\\":62},{\\\"x\\\":63},{\\\"x\\\":64},{\\\"x\\\":65},{\\\"x\\\":66},{\\\"x\\\":67},{\\\"x\\\":68},{\\\"x\\\":69},{\\\"x\\\":70},{\\\"x\\\":71},{\\\"x\\\":72},{\\\"x\\\":73},{\\\"x\\\":74},{\\\"x\\\":75},{\\\"x\\\":76},{\\\"x\\\":77},{\\\"x\\\":78},{\\\"x\\\":79},{\\\"x\\\":80}]\"]", "exact encoded result");
  return signature;
}
return run();
