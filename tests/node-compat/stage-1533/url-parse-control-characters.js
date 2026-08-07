const assert = require("node:assert");
const url = require("node:url");

const parsed = url.parse(
  "http://a.b/\tbc\ndr\ref g\"hq'j<kl>?mn\\op^q=r`99{st|uv}wz",
);
assert.strictEqual(parsed.pathname, "/%09bc%0Adr%0Def%20g%22hq%27j%3Ckl%3E");
assert.strictEqual(parsed.query, "mn%5Cop%5Eq=r%6099%7Bst%7Cuv%7Dwz");
console.log("legacy URL control characters passed");
