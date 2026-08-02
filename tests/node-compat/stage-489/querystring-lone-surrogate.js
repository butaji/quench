const assert = require("assert");
const querystring = require("querystring");

assert.strictEqual(querystring.escape("\ud800"), "%EF%BF%BD");
assert.strictEqual(
  querystring.stringify({ value: "\udfff" }),
  "value=%EF%BF%BD",
);
