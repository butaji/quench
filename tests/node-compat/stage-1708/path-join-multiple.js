const assert = require("assert");
const path = require("path");
assert.strictEqual(path.join("/a", "keys", "rsa.pem"), "/a/keys/rsa.pem");
