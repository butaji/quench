const assert = require("assert");
const { parse } = require("url");

assert.deepStrictEqual(parse(" HTTP://USER:PW@www.ExAmPlE.com "), {
  protocol: "http:",
  slashes: true,
  auth: "USER:PW",
  host: "www.example.com",
  port: null,
  hostname: "www.example.com",
  hash: null,
  search: null,
  query: null,
  pathname: "/",
  path: "/",
  href: "http://www.example.com/",
});

const backslash = parse("http:\\\\evil-phisher\\foo.html");
assert.strictEqual(backslash.host, "evil-phisher");
assert.strictEqual(backslash.pathname, "/foo.html");
assert.strictEqual(backslash.href, "http://evil-phisher/foo.html");
