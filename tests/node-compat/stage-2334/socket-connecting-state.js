const assert = require("assert");
const net = require("net");

const server = net.createServer();
server.listen(0, () => {
  const socket = net.createConnection(server.address().port);
  assert.strictEqual(socket.connecting, true);
  socket.once("connect", () => {
    assert.strictEqual(socket.connecting, false);
    socket.destroy();
    server.close(() => console.log("socket connecting state passed"));
  });
});
