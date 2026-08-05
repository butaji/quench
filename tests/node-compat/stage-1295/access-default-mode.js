const assert = require("node:assert");
const fs = require("node:fs");

fs.access(__filename, (error) => assert.strictEqual(error, null));

console.log("access default mode passed");
