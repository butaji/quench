const assert = require("assert");
const http = require("http");

assert.throws(
  () => http.request({ port: "80", headers: { host: [] } }),
  (error) => error.code === "ERR_INVALID_ARG_TYPE"
);
