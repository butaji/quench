const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve(
    "http://asdf:qwer@www.example.com",
    "http://diff:auth@www.example.com",
  ),
  "http://diff:auth@www.example.com/",
);
console.log("authority slash resolution passed");
