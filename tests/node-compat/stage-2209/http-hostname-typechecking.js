const assert = require("assert");
const http = require("http");

for (const name of ["hostname", "host"]) {
  assert.throws(
    () => http.request({ [name]: 123 }),
    (error) =>
      error.name === "TypeError" &&
      error.code === "ERR_INVALID_ARG_TYPE" &&
      error.message.includes(`options.${name}`)
  );
}
