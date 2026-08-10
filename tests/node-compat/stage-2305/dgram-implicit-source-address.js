const assert = require("assert");
const dgram = require("dgram");
const net = require("net");

const blockList = new net.BlockList();
blockList.addAddress("127.0.0.1");
const receiver = dgram.createSocket({
  type: "udp4",
  receiveBlockList: blockList,
});
receiver.on("message", () => assert.fail("blocked packet delivered"));
receiver.bind(0, "127.0.0.1", () => {
  const sender = dgram.createSocket("udp4");
  sender.send("hello", receiver.address().port, "127.0.0.1", (error) => {
    assert.ifError(error);
    sender.close();
    receiver.close();
    console.log("dgram implicit source address passed");
  });
});
