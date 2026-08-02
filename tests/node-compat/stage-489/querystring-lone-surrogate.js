const assert = require("assert");
const querystring = require("querystring");

assert.throws(() => querystring.stringify({ value: "\udfff" }), {
  code: "ERR_INVALID_URI",
  name: "URIError",
  message: "URI malformed",
});
