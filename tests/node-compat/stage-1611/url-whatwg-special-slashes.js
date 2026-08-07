const assert = require("node:assert");

const cases = [
  [
    "http:user:pass@xn--lck1c3crb1723bpq4a.com/a?a=b#c",
    "http://user:pass@xn--lck1c3crb1723bpq4a.com/a?a=b#c",
  ],
  ["https:example.com/path", "https://example.com/path"],
  ["http:example.com", "http://example.com/"],
  ["ftp:example.com/path", "ftp://example.com/path"],
];
for (const [input, expected] of cases) {
  assert.strictEqual(new URL(input).href, expected, input);
}
console.log("WHATWG special slash matrix passed");
