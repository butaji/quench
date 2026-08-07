const assert = require("assert");
const crypto = require("crypto");

assert.throws(
  () => crypto.createHmac("sha256", "a secret").update("0", "hex"),
  { code: "ERR_INVALID_ARG_VALUE", name: "TypeError" },
);
