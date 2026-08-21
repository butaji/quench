const assert = require("assert");
const http = require("http");

for (let index = 0; index <= 32; index += 1) {
  const path = `bad${String.fromCharCode(index)}path`;
  assert.throws(
    () => http.get({ path }, () => {}),
    (error) =>
      error.name === "TypeError" &&
      error.code === "ERR_UNESCAPED_CHARACTERS" &&
      error.message === "Request path contains unescaped characters",
  );
}
