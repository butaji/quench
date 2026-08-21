const assert = require("assert");
const fs = require("fs");

assert.rejects(fs.promises.access(__filename, 8), {
  code: "ERR_OUT_OF_RANGE",
});
