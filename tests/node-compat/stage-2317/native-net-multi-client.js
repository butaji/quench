const assert = require("assert");
const net = require("net");

const received = [];
const server = net.createServer((socket) => {
  socket.on("data", (chunk) => {
    received.push(chunk.toString());
    socket.end();
    if (received.length === 3) {
      assert.deepStrictEqual(received.sort(), ["one", "three", "two"]);
      server.close(() => console.log("native net multi-client passed"));
    }
  });
});

server.listen(
  { port: 0, host: "127.0.0.1", __quenchNativeTransport: true },
  () => {
    const port = server.address().port;
    for (const value of ["one", "two", "three"]) {
      const client = net.createConnection(
        { host: "127.0.0.1", port, __quenchNativeTransport: true },
        () => client.end(value)
      );
    }
  }
);
