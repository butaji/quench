const assert = require("assert");
const net = require("net");

const server = net.createServer((socket) => {
  socket.on("data", (chunk) => {
    assert.strictEqual(chunk.toString(), "ping");
    socket.end("pong");
  });
});
const lookup = (host, options, callback) => {
  assert.strictEqual(host, "example.org");
  if (options.all) {
    callback(null, [
      { address: "::1", family: 6 },
      { address: "127.0.0.1", family: 4 },
    ]);
  } else callback(null, "::1", 6);
};
server.listen(0, "127.0.0.1", () => {
  const socket = net.createConnection({
    host: "example.org",
    port: server.address().port,
    lookup,
    autoSelectFamily: true,
  });
  socket.setEncoding("utf8");
  socket.on("connect", () => socket.write("ping"));
  socket.on("data", (value) => assert.strictEqual(value, "pong"));
  socket.on("end", () => {
    socket.destroy();
    server.close();
    console.log("net auto select family lookup passed");
  });
  socket.on("error", assert.fail);
});
