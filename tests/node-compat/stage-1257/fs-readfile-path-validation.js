const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(
  () =>
    fs.readFile(
      () => {},
      () => {},
    ),
  {
    code: "ERR_INVALID_ARG_TYPE",
  },
);

console.log("readFile path validation passed");
