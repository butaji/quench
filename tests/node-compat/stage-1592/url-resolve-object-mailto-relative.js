const assert = require("node:assert");
const url = require("node:url");

const cases = [
  [
    "mailto:local",
    "local/qual@domain.org#frag",
    "mailto:local/qual@domain.org#frag",
  ],
  [
    "mailto:local/qual1@domain1.org",
    "more/qual2@domain2.org#frag",
    "mailto:local/more/qual2@domain2.org#frag",
  ],
  ["mailto:local@domain?query1", "?query2", "mailto:local@domain?query2"],
  ["mailto:local@domain?query1", "#frag", "mailto:local@domain?query1#frag"],
  [
    "mailto:local@domain",
    "local2@domain2?query2",
    "mailto:local2@domain2?query2",
  ],
  ["mailto:", "local@domain?query2", "mailto:local@domain?query2"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("mailto relative matrix passed");
