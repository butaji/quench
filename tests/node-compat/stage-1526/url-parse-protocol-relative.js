const assert = require("node:assert");
const url = require("node:url");

assert.deepStrictEqual(url.parse("//some_path"), {
  href: "//some_path",
  pathname: "//some_path",
  path: "//some_path",
  protocol: null,
  slashes: null,
  auth: null,
  host: null,
  port: null,
  hostname: null,
  hash: null,
  search: null,
  query: null,
});
console.log("protocol-relative legacy URL parsing passed");
