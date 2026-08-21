const assert = require("assert");
const net = require("net");

const server = net.createServer((socket) => {
  socket.once("close", () => server.close());
});
server.listen(0, () => {
  const client = net.connect(server.address().port);
  client.once("connect", () => client.destroy());
});

server.once("close", () => assert.ok(true));
