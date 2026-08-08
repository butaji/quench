const assert = require("assert");
const net = require("net");

const server = net.createServer(
  { keepAlive: true, keepAliveInitialDelay: 3000 },
  (socket) => {
    assert.strictEqual(socket._keepAlive, true);
    assert.strictEqual(socket._keepAliveDelay, 3);
    socket.destroy();
    server.close(() => console.log("server keepalive defaults passed"));
  }
);
server.listen(0, () => net.createConnection(server.address().port));
