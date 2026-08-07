const assert = require("assert");
const fs = require("fs");
assert.throws(() => fs.readFileSync({}), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
});
