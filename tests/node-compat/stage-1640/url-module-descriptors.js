const assert = require("node:assert");
const { URL } = require("node:url");
for (
  const name of [
    "href",
    "protocol",
    "username",
    "password",
    "host",
    "hostname",
    "port",
    "pathname",
    "search",
    "hash",
    "origin",
    "searchParams",
  ]
) {
  assert.notStrictEqual(
    Object.getOwnPropertyDescriptor(URL.prototype, name),
    undefined,
    name,
  );
}
console.log("URL module descriptors passed");
