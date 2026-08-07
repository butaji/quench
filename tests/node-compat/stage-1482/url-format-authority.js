const assert = require("node:assert");
const url = require("node:url");

const formatted = url.format(
  new URL("http://user:pass@xn--lck1c3crb1723bpq4a.com/a?a=b#c"),
);

assert.strictEqual(
  formatted,
  "http://user:pass@xn--lck1c3crb1723bpq4a.com/a?a=b#c",
);
console.log("url format authority passed");
