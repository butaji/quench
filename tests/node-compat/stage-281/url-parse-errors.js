const assert = require("assert");
const { parse } = require("url");

assert.throws(
  () => parse("http://%E0%A4%A@fail"),
  (error) => {
    return error instanceof URIError && error.code === undefined;
  },
);

assert.throws(() => parse("http://[127.0.0.1\\x00c8763]:8000/"), {
  code: "ERR_INVALID_URL",
  input: "http://[127.0.0.1\\x00c8763]:8000/",
});

for (
  const value of [
    "https://evil.com:.example.com",
    "git+ssh://git@github.com:npm/npm",
  ]
) {
  assert.throws(() => parse(value), { code: "ERR_INVALID_ARG_VALUE" });
}
