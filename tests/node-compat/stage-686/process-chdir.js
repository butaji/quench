"use strict";

const assert = require("assert");
const processApi = require("process");

const current = processApi.cwd();
assert.strictEqual(processApi.chdir(current), undefined);
assert.strictEqual(processApi.cwd(), current);

console.log("process chdir passed");
