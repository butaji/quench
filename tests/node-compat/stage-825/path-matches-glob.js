"use strict";

const assert = require("assert");
const pathApi = require("node:path");

assert.strictEqual(typeof pathApi.matchesGlob, "function");
assert.strictEqual(pathApi.matchesGlob("a.js", "*.js"), true);
assert.strictEqual(pathApi.matchesGlob("a.txt", "*.js"), false);

console.log("path matches glob passed");
