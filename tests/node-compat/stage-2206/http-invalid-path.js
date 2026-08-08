const assert = require("assert");
const http = require("http");

assert.throws(
  () => http.request({ path: "/thisisinvalid\uffe2" }),
  (error) =>
    error.name === "TypeError" && error.code === "ERR_UNESCAPED_CHARACTERS"
);
