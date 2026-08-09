"use strict";

const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
assert.strictEqual(socket.type, "udp4");
socket.bind(0, "127.0.0.1", () => {
  assert.strictEqual(socket.address().family, "IPv4");
  socket.send(Buffer.from("packet"), 9999, "127.0.0.1", (error) => {
    assert.ifError(error);
    socket.close();
  });
});
assert.strictEqual(typeof socket.unref, "function");

console.log("dgram passed");
