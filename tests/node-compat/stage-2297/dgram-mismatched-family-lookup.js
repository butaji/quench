const assert = require("assert");
const dgram = require("dgram");
const dns = require("dns");

const originalLookup = dns.lookup;
let called = false;
dns.lookup = (host, family, callback) => {
  called = true;
  assert.strictEqual(host, "::1");
  assert.strictEqual(family, 4);
  callback(null, "127.0.0.1", 4);
};

const socket = dgram.createSocket("udp4");
socket.bind(0, "::1", () => {
  assert.strictEqual(called, true);
  dns.lookup = originalLookup;
  socket.close();
  console.log("dgram mismatched-family lookup passed");
});
