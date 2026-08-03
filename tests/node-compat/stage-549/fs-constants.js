"use strict";

const assert = require("assert");
const fs = require("fs");

assert.strictEqual(fs.constants.O_RDONLY, 0);
assert.strictEqual(fs.constants.O_CREAT, 64);
assert.strictEqual(fs.constants.COPYFILE_EXCL, 1);
assert.strictEqual(Object.isFrozen(fs.constants), true);

console.log("fs constants passed");
