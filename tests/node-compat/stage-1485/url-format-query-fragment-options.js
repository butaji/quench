const assert = require("node:assert");
const url = require("node:url");

const parsed = new URL("http://user:pass@xn--lck1c3crb1723bpq4a.com/a?a=b#c");
assert.strictEqual(
  url.format(parsed, { fragment: false }),
  "http://user:pass@xn--lck1c3crb1723bpq4a.com/a?a=b",
);
assert.strictEqual(
  url.format(parsed, { search: false }),
  "http://user:pass@xn--lck1c3crb1723bpq4a.com/a#c",
);
console.log("url format query and fragment options passed");
