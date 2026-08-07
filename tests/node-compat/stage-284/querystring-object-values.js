const assert = require("assert");
const querystring = require("querystring");

assert.strictEqual(
  querystring.stringify({ date: new Date(), regexp: /x/, fn: () => {} }),
  "date=&regexp=&fn=",
);
assert.strictEqual(
  querystring.stringify({ value: false, count: 2n }),
  "value=false&count=2",
);
