const assert = require("node:assert");
const url = new URL("http://user:pass@foo.bar.com:21/aaa/zzz?l=24#test");
const properties = [];
for (const property in url) properties.push(property);
assert.deepStrictEqual(properties, [
  "toString",
  "href",
  "origin",
  "protocol",
  "username",
  "password",
  "host",
  "hostname",
  "port",
  "pathname",
  "search",
  "searchParams",
  "hash",
  "toJSON",
]);
console.log("URL enumerable properties inspected");
