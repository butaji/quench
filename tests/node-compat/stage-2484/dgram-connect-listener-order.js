const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
const events = [];
socket.once("connect", () => events.push("listener"));
socket.connect(12345, () => events.push("callback"));

queueMicrotask(() => {
  assert.deepStrictEqual(events, ["listener", "callback"]);
  assert.deepStrictEqual(socket.remoteAddress(), {
    address: "127.0.0.1",
    family: "IPv4",
    port: 12345,
  });
  socket.close();
});
