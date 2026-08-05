const assert = require("node:assert");
const { URL } = require("node:url");
const url = new URL("https://github.com/");
assert.throws(() => {
  url.href = Symbol();
}, /Cannot convert a Symbol value to a string/);
console.log("URL setter symbol rejection passed");
