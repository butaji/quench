const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.bind();
let listening = false;
const timer = setTimeout(() => socket.close(), 50);
socket.on("listening", () => {
  clearTimeout(timer);
  listening = true;
  socket.close();
});
socket.on("close", () => {
  assert.strictEqual(listening, true);
  console.log("dgram listening after bind passed");
});
