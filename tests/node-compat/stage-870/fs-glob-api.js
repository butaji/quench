"use strict";

const assert = require("assert");
const fs = require("node:fs");

assert.strictEqual(typeof fs.glob, "function");
assert.strictEqual(typeof fs.promises.glob, "function");

console.log("fs glob api passed");
