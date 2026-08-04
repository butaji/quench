const assert = require("assert");
const util = require("util");

assert.strictEqual(util.format(1, 5n), "1 5n");
