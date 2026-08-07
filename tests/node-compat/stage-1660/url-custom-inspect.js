const assert = require("node:assert");

const url = new URL(
  "https://username:password@host.name:8080/path/name/?que=ry#hash",
);
assert.strictEqual(
  require("util").inspect(url),
  "URL {\n" +
    "  href: 'https://username:password@host.name:8080/path/name/?que=ry#hash',\n" +
    "  origin: 'https://host.name:8080',\n" +
    "  protocol: 'https:',\n" +
    "  username: 'username',\n" +
    "  password: 'password',\n" +
    "  host: 'host.name:8080',\n" +
    "  hostname: 'host.name',\n" +
    "  port: '8080',\n" +
    "  pathname: '/path/name/',\n" +
    "  search: '?que=ry',\n" +
    "  searchParams: URLSearchParams { 'que' => 'ry' },\n" +
    "  hash: '#hash'\n" +
    "}",
);
console.log("URL custom inspect passed");
