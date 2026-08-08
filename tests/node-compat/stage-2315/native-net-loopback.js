const assert = require("assert");
const net = require("net");

const server = net.createServer((socket) => {
  socket.on("data", (chunk) => {
    assert.strictEqual(chunk.toString(), "ping");
    socket.write("pong");
  });
});

server.listen(
  { port: 0, host: "127.0.0.1", __quenchNativeTransport: true },
  () => {
    const address = server.address();
    assert.ok(address.port > 0);
    const client = net.createConnection(
      {
        host: "127.0.0.1",
        port: address.port,
        __quenchNativeTransport: true
      },
      () => client.write("ping")
    );
    client.on("data", (chunk) => {
      assert.strictEqual(chunk.toString(), "pong");
      client.destroy();
      server.close(() => console.log("native net loopback passed"));
    });
  }
);
