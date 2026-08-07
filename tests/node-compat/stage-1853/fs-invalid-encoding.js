const assert = require("assert");
const fs = require("fs");

assert.throws(() => fs.readFile("/tmp/missing", "test", () => {}), {
  name: "TypeError",
  code: "ERR_INVALID_ARG_VALUE",
});
assert.throws(() => fs.readFileSync("/tmp/missing", "test"), {
  name: "TypeError",
  code: "ERR_INVALID_ARG_VALUE",
});
assert.throws(() => fs.readdir("/tmp/missing", "test", () => {}), {
  name: "TypeError",
  code: "ERR_INVALID_ARG_VALUE",
});
assert.throws(() => fs.readdirSync("/tmp/missing", "test"), {
  name: "TypeError",
  code: "ERR_INVALID_ARG_VALUE",
});
assert.throws(() => fs.readlink("/tmp/missing", "test", () => {}), {
  name: "TypeError",
  code: "ERR_INVALID_ARG_VALUE",
});
assert.throws(() => fs.readlinkSync("/tmp/missing", "test"), {
  name: "TypeError",
  code: "ERR_INVALID_ARG_VALUE",
});
console.log("fs invalid encoding validation passed");
