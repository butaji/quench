const assert = require("node:assert");
const url = require("node:url");

const options = url.urlToHttpOptions(
  new URL("http://user:pass@foo.bar.com:21/aaa/zzz?l=24#test"),
);
assert.strictEqual(options.protocol, "http:");
assert.strictEqual(options.auth, "user:pass");
assert.strictEqual(options.hostname, "foo.bar.com");
assert.strictEqual(options.port, 21);
assert.strictEqual(options.path, "/aaa/zzz?l=24");
console.log("url to HTTP options passed");
