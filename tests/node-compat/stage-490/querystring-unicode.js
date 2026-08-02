const assert = require("assert");
const querystring = require("querystring");

assert.strictEqual(querystring.unescape("%F0%9F%98%80"), "😀");
assert.strictEqual(querystring.unescape("😀"), "😀");
assert.deepStrictEqual(querystring.parse("value=😀"), { value: "😀" });
