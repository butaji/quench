const assert = require("assert");
const dgram = require("dgram");
const dns = require("dns");

assert.strictEqual(require("dns"), dns);

let called = false;
const originalLookup = dns.lookup;
dns.lookup = (host, family, callback) => {
  called = true;
  assert.strictEqual(host, "example.invalid");
  assert.strictEqual(family, 4);
  callback(null, "127.0.0.1", 4);
};

const socket = dgram.createSocket("udp4");
socket.bind(0, "example.invalid", () => {
  assert.strictEqual(called, true);
  dns.lookup = originalLookup;
  socket.close();
  console.log("dns module identity passed");
});
