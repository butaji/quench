const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.format(new URL("http://user:pass@xn--lck1c3crb1723bpq4a.com/a?a=b#c"), {
    unicode: true,
  }),
  "http://user:pass@理容ナカムラ.com/a?a=b#c",
);
console.log("url format unicode option passed");
