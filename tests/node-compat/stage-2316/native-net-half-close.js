const assert = require("assert");
const net = require("net");

let serverEnded = false;
const server = net.createServer((socket) => {
  socket.once("end", () => {
    serverEnded = true;
    socket.destroy();
    server.close(() => console.log("native net half-close passed"));
  });
});

server.listen(
  { port: 0, host: "127.0.0.1", __quenchNativeTransport: true },
  () => {
    const client = net.createConnection(
      {
        port: server.address().port,
        host: "127.0.0.1",
        __quenchNativeTransport: true
      },
      () => client.end()
    );
  }
);

process.on("exit", () => assert.ok(serverEnded));
