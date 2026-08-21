const assert = require("assert");
const http = require("http");
const net = require("net");

const server = http.createServer();
server.on("connection", (socket) => {
  assert.strictEqual(typeof socket.write, "function");
  socket.destroy();
  server.close(() => console.log("http net connection bridge passed"));
});
server.listen(0, () => {
  net.createConnection(server.address().port, "127.0.0.1");
});
