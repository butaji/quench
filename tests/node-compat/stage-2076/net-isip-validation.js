const assert = require("assert");
const net = require("net");

const valid = [
  "127.0.0.1",
  "0000:0000:0000:0000:0000:0000:0000:0000",
  "1050:0:0:0:5:600:300c:326b",
  "2001:252:0:1::2008:6",
  "::2001:252:1:255.255.255.255",
  "fe80::2008%eth0",
];
for (const value of valid) assert.notStrictEqual(net.isIP(value), 0, value);

const invalid = [
  "x127.0.0.1",
  "0000:0000:0000:0000:0000:0000:0000::0000",
  ":2001:252:0:1::2008:6",
  "2001:252:0:1::2008:6:",
  "2001:252:1::255.255.255.255.76",
  "2001:252::1::2008:6",
  "0000:0000:0000:0000:0000:0000:12345:0000",
];
for (const value of invalid) {
  const actual = net.isIP(value);
  assert.strictEqual(actual, 0, `${value}: ${actual}`);
}
