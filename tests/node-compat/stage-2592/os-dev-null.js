const assert = require("assert");
const os = require("os");

assert.strictEqual(os.devNull, "/dev/null");
console.log("os devNull passed");
