const assert = require("assert");
const fs = require("fs");
assert.throws(() => fs.readFileSync(__filename, "not-an-encoding"), {
  code: "ERR_UNKNOWN_ENCODING",
  name: "TypeError",
});
