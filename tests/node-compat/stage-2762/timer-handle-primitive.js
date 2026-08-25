"use strict";
const assert = require("assert");
const timer = setTimeout(() => {}, 1000);
const primitive = timer[Symbol.toPrimitive]();
assert.strictEqual(typeof primitive, "number");
assert.strictEqual(+timer, primitive);
assert.strictEqual(String(timer), String(primitive));
clearTimeout(primitive);
