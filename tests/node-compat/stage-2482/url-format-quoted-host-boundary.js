const assert = require("assert");
const { format } = require("url");

const cases = [
  [
    'http://google.com" onload="alert(42)',
    "http://google.com/%22%20onload=%22alert(42)",
  ],
  ['https://example.com"x', "https://example.com/%22x"],
  ['ftp://example.com"x', "ftp://example.com/%22x"],
  ['http://google.com/path"x', "http://google.com/path%22x"],
];

for (const [input, expected] of cases) {
  assert.strictEqual(format(input), expected);
}
