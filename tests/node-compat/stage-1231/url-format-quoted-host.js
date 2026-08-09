const assert = require("assert");
const { format } = require("url");

assert.strictEqual(
  format('http://google.com" onload="alert(42)/'),
  "http://google.com/%22%20onload=%22alert(42)/"
);
