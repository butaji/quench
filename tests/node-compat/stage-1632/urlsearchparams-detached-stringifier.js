const assert = require("node:assert");

const toString = URLSearchParams.prototype.toString;
assert.throws(() => toString.call({}), {
  code: "ERR_INVALID_THIS",
  name: "TypeError",
});
console.log("URLSearchParams detached stringifier passed");
