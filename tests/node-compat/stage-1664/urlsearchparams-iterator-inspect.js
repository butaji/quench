const assert = require("node:assert");

const util = require("node:util");
const params = new URLSearchParams("?a=a&b=b&b=c");
const iterator = params.entries();
assert.strictEqual(
  util.inspect(iterator),
  "URLSearchParams Iterator { [ 'a', 'a' ], [ 'b', 'b' ], [ 'b', 'c' ] }",
);
iterator.next();
assert.strictEqual(
  util.inspect(iterator),
  "URLSearchParams Iterator { [ 'b', 'b' ], [ 'b', 'c' ] }",
);
console.log("URLSearchParams iterator inspection passed");
