"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.ref, "function");
assert.strictEqual(typeof processApi.unref, "function");
assert.strictEqual(processApi.ref({}), undefined);
assert.strictEqual(processApi.unref({}), undefined);

console.log("process ref methods passed");
