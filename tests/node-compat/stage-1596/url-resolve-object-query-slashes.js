const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["http://ex/x/y?pp/qq", "?pp/rr", "http://ex/x/y?pp/rr"],
  ["http://ex/x/y?pp/qq", "y/z", "http://ex/x/y/z"],
  ["http://ex/x/y?q", "y?q", "http://ex/x/y?q"],
  ["http://ex", "/x/y?q", "http://ex/x/y?q"],
  ["http://ex/x/y", "?q", "http://ex/x/y?q"],
  ["http://ex/x/y", "#frag", "http://ex/x/y#frag"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("query slash matrix passed");
