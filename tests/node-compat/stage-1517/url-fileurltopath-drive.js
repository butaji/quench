const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.fileURLToPath("file:///C:/foo"), "C:\\foo");
console.log("url fileURLToPath drive passed");
