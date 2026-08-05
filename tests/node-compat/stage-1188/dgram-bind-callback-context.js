const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.bind(0, function () {
  assert.strictEqual(this, socket);
  assert.strictEqual(this.address().address, "0.0.0.0");
  assert.ok(this.address().port > 0);
  this.close();
});

const ipv6 = dgram.createSocket("udp6");
ipv6.bind(0, function () {
  assert.strictEqual(this.address().address, "::");
  this.close();
});
