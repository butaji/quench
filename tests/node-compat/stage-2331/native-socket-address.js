const assert = require("assert");
const net = require("net");

const server = net.createServer((socket) => {
  assert.strictEqual(socket.remoteAddress, "127.0.0.1");
  assert.ok(socket.remotePort > 0);
});
server.listen(
  { port: 0, host: "127.0.0.1", __quenchNativeTransport: true },
  () => {
    const client = net.createConnection(
      {
        host: "127.0.0.1",
        port: server.address().port,
        __quenchNativeTransport: true
      },
      () => {
        assert.strictEqual(client.localAddress, "127.0.0.1");
        assert.ok(client.localPort > 0);
        assert.strictEqual(client.remoteAddress, "127.0.0.1");
        assert.strictEqual(client.remotePort, server.address().port);
        assert.deepStrictEqual(client.address(), {
          address: "127.0.0.1",
          family: "IPv4",
          port: client.localPort
        });
        client.destroy();
        server.close(() => console.log("native socket address passed"));
      }
    );
  }
);
