const assert = require("assert");
const http = require("http");

assert.throws(() => new http.Agent({ maxTotalSockets: "test" }), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError"
});

for (const value of [-1, 0, NaN]) {
  assert.throws(() => new http.Agent({ maxTotalSockets: value }), {
    code: "ERR_OUT_OF_RANGE",
    name: "RangeError"
  });
}

assert.ok(new http.Agent({ maxTotalSockets: Infinity }));
console.log("http agent max total sockets validation passed");
