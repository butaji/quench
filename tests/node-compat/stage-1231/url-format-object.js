const assert = require("assert");
const { format } = require("url");

assert.strictEqual(
  format({
    protocol: "http",
    host: "a.com",
    pathname: "a/b/c",
    hash: "h",
    search: "s",
  }),
  "http://a.com/a/b/c?s#h",
);
