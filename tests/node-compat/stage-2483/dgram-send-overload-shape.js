const assert = require("assert");
const dgram = require("dgram");

const receiver = dgram.createSocket("udp4");
const sender = dgram.createSocket("udp4");
receiver.bind(0, () => {
  sender.bind(0, () => {
    const port = receiver.address().port;
    receiver.once("message", (message) => {
      assert.strictEqual(message.toString(), "bc");
      receiver.close();
      sender.close();
    });
    sender.send(Buffer.from("abcd"), 1, 2, port);
  });
});

const invalid = dgram.createSocket("udp4");
assert.throws(() => invalid.send(Buffer.from("x"), 40000, 1), {
  code: "ERR_INVALID_ARG_TYPE",
});
invalid.close();
