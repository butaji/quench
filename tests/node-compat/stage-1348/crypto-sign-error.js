const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(
  () => crypto.createSign("SHA256").update("test").sign("small-key"),
  {
    name: "Error",
    message: "error:02000070:rsa routines::digest too big for rsa key",
    library: "rsa routines",
  },
);
console.log("crypto sign error passed");
