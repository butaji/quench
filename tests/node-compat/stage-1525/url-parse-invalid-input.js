const assert = require("node:assert");
const url = require("node:url");

assert.throws(() => url.parse([]), {
  code: "ERR_INVALID_ARG_TYPE",
  message:
    'The "url" argument must be of type string. Received an instance of Array',
});
assert.throws(() => url.parse(() => {}), {
  code: "ERR_INVALID_ARG_TYPE",
  message: 'The "url" argument must be of type string. Received function ',
});
assert.throws(() => url.parse("http://[127.0.0.1\u0000c8763]:8000/"), {
  code: "ERR_INVALID_URL",
  input: "http://[127.0.0.1\u0000c8763]:8000/",
});
assert.throws(() => url.parse("https://evil.com:.example.com"), {
  code: "ERR_INVALID_ARG_VALUE",
});
console.log("invalid legacy URL inputs passed");
