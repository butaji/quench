const assert = require("assert");
const util = require("util");
assert.doesNotThrow(() => util.inspect({ value: 1 }, null, 2, false));
assert.doesNotThrow(() => util.inspect({ value: 1 }, false, 2, false));
console.log("util inspect legacy arguments passed");
