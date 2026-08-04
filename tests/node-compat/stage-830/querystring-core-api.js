"use strict";

const assert = require("assert");
const querystringApi = require("node:querystring");

for (const name of ["parse", "stringify", "escape", "unescape"]) {
  assert.strictEqual(typeof querystringApi[name], "function");
}
assert.strictEqual(querystringApi.stringify({ value: "ok" }), "value=ok");
assert.deepStrictEqual(querystringApi.parse("value=ok"), { value: "ok" });

console.log("querystring core api passed");
