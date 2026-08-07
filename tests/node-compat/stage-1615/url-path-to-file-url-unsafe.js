const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.pathToFileURL('/foo\r\n\t<>"#%{}|^[\\~]`?bar').href,
  "file:///foo%0D%0A%09%3C%3E%22%23%25%7B%7D%7C%5E%5B%5C%7E%5D%60%3Fbar",
);
console.log("unsafe path encoding passed");
