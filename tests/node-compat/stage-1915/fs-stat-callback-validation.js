const assert = require("assert");
const fs = require("fs");

for (const callback of [null, true, false, 0, 1, "foo", /foo/, [], {}]) {
  assert.throws(() => fs.stat(__filename, callback), {
    name: "TypeError",
    code: "ERR_INVALID_ARG_TYPE",
  });
}
console.log("fs stat callback validation passed");
