const assert = require("node:assert");
const url = require("node:url");

assert.throws(() => url.fileURLToPath("https://a/b/c"), {
  code: "ERR_INVALID_URL_SCHEME",
});
console.log("url file scheme error passed");
