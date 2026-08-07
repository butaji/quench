const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.appendFile("append.txt", console.log), {
  code: "ERR_INVALID_ARG_TYPE",
});

console.log("appendFile validation passed");
