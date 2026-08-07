const assert = require("node:assert");
const url = require("node:url");

const parsed = new URL("http://user:pass@xn--lck1c3crb1723bpq4a.com/a?a=b#c");
assert.strictEqual(
  url.format(parsed, { auth: false }),
  "http://xn--lck1c3crb1723bpq4a.com/a?a=b#c",
);
console.log("url format auth option passed");
