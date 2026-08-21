const assert = require("assert");
const dgram = require("dgram");
const net = require("net");

const blockList = new net.BlockList();
blockList.addAddress("127.0.0.1");
const socket = dgram.createSocket({ type: "udp4", sendBlockList: blockList });
socket.connect(9999, "127.0.0.1", (error) => {
  assert.strictEqual(error.code, "ERR_IP_BLOCKED");
  socket.close();
  console.log("dgram send blocklist passed");
});
