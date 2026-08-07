const assert = require("assert");
const http = require("http");

assert.strictEqual(http.maxHeaderSize, 16 * 1024);
console.log("http maxHeaderSize passed");
