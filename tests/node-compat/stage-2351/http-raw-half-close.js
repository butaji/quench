const assert = require("assert");
const http = require("http");
const net = require("net");

let handled = 0;
const server = http.createServer((request, response) => {
  handled++;
  response.end("done");
});
server.httpAllowHalfOpen = true;
server.listen(0, () => {
  const socket = net.createConnection(server.address().port, "127.0.0.1");
  let output = "";
  socket.on("data", (chunk) => (output += chunk.toString()));
  socket.on("end", () => {
    assert.strictEqual(handled, 1);
    assert.match(output, /HTTP\/1\.1 200 OK/);
    server.close(() => console.log("http raw half-close passed"));
  });
  socket.on("connect", () => {
    socket.write("GET /half HTTP/1.1\r\nHost: example.com\r\n\r\n");
    socket.end();
    assert.strictEqual(socket.readyState, "readOnly");
  });
});
