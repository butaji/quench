const assert = require("assert");
const querystring = require("querystring");

const previous = querystring.unescape;
querystring.unescape = (value) => value.replace(/o/g, "_");
assert.deepStrictEqual(querystring.parse("foo=bor"), { f__: "b_r" });
querystring.unescape = previous;
