const dgram = require("dgram");

const receiver = dgram.createSocket("udp4");
receiver.bind(0, "localhost", () => {
  const { address, port } = receiver.address();
  receiver.close(() => {
    const sender = dgram.createSocket("udp4");
    sender.send("payload", port, address, () => {
      throw new Error("closed destination callback should be suppressed");
    });
    setImmediate(() => {
      sender.close();
      console.log("dgram closed-target callback passed");
    });
  });
});
