const assert = require("assert");
const url = require("url");

assert.deepStrictEqual(
  url.parse(" HTTP://USER:PW@www.ExAmPlE.com "),
  Object.assign(new url.Url(), {
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
    href: "http://USER:PW@www.example.com/"
  })
);

const backslash = url.parse("http:\\\\evil-phisher\\foo.html");
assert.strictEqual(backslash.host, "evil-phisher");
assert.strictEqual(backslash.pathname, "/foo.html");
assert.strictEqual(backslash.href, "http://evil-phisher/foo.html");
