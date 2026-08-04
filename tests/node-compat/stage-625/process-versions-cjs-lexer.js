"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.cjs_module_lexer, "string");
assert(/^\d+\.\d+\.\d+/.test(processApi.versions.cjs_module_lexer));

console.log("process versions cjs lexer passed");
