const assert = require("assert");
const http = require("http");

for (const value of ["foo", 42, true, []]) {
  assert.throws(() => new http.Server(value), {
    code: "ERR_INVALID_ARG_TYPE"
  });
}
assert.ok(new http.Server());
console.log("http server options passed");
