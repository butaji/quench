const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(
  () => crypto.createSign("SHA1").sign({ key: "PRIVATE KEY", padding: 4 }),
  {
    message:
      "error:1C8000A5:Provider routines::illegal or unsupported padding mode",
  },
);
console.log("crypto sign padding error passed");
