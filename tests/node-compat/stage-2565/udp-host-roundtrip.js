const assert = require("assert");

const receiverPort = 45123;
const receiver = __quench_udp_socket("127.0.0.1", receiverPort);
const sender = __quench_udp_socket("127.0.0.1", 0);
const sent = __quench_udp_send(sender, "127.0.0.1", receiverPort, [1, 2, 3]);
assert.strictEqual(sent, 3);
let received = [];
for (let attempt = 0; attempt < 100 && received.length === 0; attempt++) {
  received = __quench_udp_recv(receiver);
}
assert.deepStrictEqual(received, [1, 2, 3]);
__quench_udp_close(sender);
__quench_udp_close(receiver);
console.log("UDP host boundary passed");
