const assert = require("node:assert");
const url = require("node:url");

const cases = [
  [
    "file:/devel/WWW/2000/10/swap/test/reluri-1.n3",
    "file://meetings.example.com/cal#m1",
    "file://meetings.example.com/cal#m1",
  ],
  [
    "file:/home/connolly/w3ccvs/WWW/2000/10/swap/test/reluri-1.n3",
    "file://meetings.example.com/cal#m1",
    "file://meetings.example.com/cal#m1",
  ],
  ["file:/ex/x/y", "ftp://ex/x/q/r", "ftp://ex/x/q/r"],
  ["file:/example2/x/y/z", "/example/x/abc", "file:/example/x/abc"],
  ["file:/ex/x/y/z", "../r", "file:/ex/x/r"],
  ["file:/ex/x/y/z", "/r", "file:/r"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("file authority matrix passed");
