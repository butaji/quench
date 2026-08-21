const assert = require("assert");
const debug = require("debug")("quench:application");

assert.strictEqual(typeof debug, "function");
debug("application probe");
console.log("npm debug application passed");
