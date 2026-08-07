const assert = require("node:assert");

const cases = [
  ["", undefined],
  ["test", undefined],
  [undefined, undefined],
  [0, undefined],
  [true, undefined],
  [false, undefined],
  [null, undefined],
  [new Date(), undefined],
  [new RegExp(), undefined],
  ["test", null],
  ["http://nodejs.org", null],
  [() => {}, undefined],
];
for (const [input, base] of cases) {
  assert.throws(() => new URL(input, base), { name: "TypeError" });
}
console.log("URL invalid input matrix passed");
