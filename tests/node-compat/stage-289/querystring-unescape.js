const assert = require("assert");
const querystring = require("querystring");

assert.strictEqual(
  querystring.unescape("there%20are%20spaces"),
  "there are spaces",
);
assert.strictEqual(
  querystring.unescape("there%2Qare%0-fake%escaped"),
  "there%2Qare%0-fake%escaped",
);
assert.strictEqual(querystring.unescape("%%2a"), "%*");
assert.strictEqual(querystring.unescape("%2sf%2a"), "%2sf*");
assert.strictEqual(querystring.unescape("%2%2af%2a"), "%2*f*");
