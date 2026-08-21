const assert = require("assert");
const dgram = require("dgram");

const sender = dgram.createSocket("udp4");
const receiver = dgram.createSocket("udp4");
receiver.bind(0, "localhost", () => {
  const { port, address } = receiver.address();
  receiver.close(() => {
    sender.send(Buffer.from("x"), 0, 1, port, address, () => {
      assert.fail("callback should not run after destination close");
    });
    setTimeout(() => {
      sender.close();
      console.log("dgram closed destination callback passed");
    }, 10);
  });
});
