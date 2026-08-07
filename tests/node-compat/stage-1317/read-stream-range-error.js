const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.createReadStream(__filename, { start: 10, end: 2 }), {
  code: "ERR_OUT_OF_RANGE",
  name: "RangeError",
});
console.log("read stream range error passed");
