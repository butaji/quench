"use strict";

const assert = require("assert");
const constants = require("constants");

assert.strictEqual(constants.O_RDONLY, 0);
assert.strictEqual(constants.O_CREAT, 64);
assert.strictEqual(constants.S_IFREG, 0o100000);
assert.strictEqual(constants.SIGTERM, 15);
assert.strictEqual(constants.COPYFILE_EXCL, 1);
assert.strictEqual(Object.isFrozen(constants), true);

console.log("constants passed");
