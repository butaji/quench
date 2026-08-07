const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["http://ex/x/y", "./q:r", "http://ex/x/q:r"],
  ["http://ex/x/y", "./p=q:r", "http://ex/x/p=q:r"],
  ["http://ex/x/y?pp/qq", "?pp/rr", "http://ex/x/y?pp/rr"],
  ["http://ex/x/y?pp/qq", "y/z", "http://ex/x/y/z"],
  ["http://ex/x/y?q", "y?q", "http://ex/x/y?q"],
  ["http://ex?p", "/x/y?q", "http://ex/x/y?q"],
  ["http://ex/x/y", "q/r#s", "http://ex/x/q/r#s"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("colon relative matrix passed");
