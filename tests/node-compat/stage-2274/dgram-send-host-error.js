const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.send("x", 100, "dne.example.com", (error) => {
  assert.strictEqual(error.code, "ENOTFOUND");
  socket.close();
  console.log("dgram send host error passed");
});
