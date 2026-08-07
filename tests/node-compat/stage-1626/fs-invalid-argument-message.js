const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.accessSync(true), {
  code: "ERR_INVALID_ARG_TYPE",
  message: /Received type boolean \(true\)/,
});
console.log("Filesystem invalid argument message passed");
