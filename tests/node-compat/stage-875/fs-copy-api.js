"use strict";

const assert = require("assert");
const fs = require("node:fs");

assert.strictEqual(typeof fs.cp, "function");
assert.strictEqual(typeof fs.cpSync, "function");
assert.strictEqual(typeof fs.promises.cp, "function");

console.log("fs copy api passed");
