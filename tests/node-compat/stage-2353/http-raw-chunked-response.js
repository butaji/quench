const assert = require("assert");
const http = require("http");
const net = require("net");

const server = http.createServer((request, response) => {
  response.write("hello");
  response.end(" world");
});
server.listen(0, () => {
  const socket = net.createConnection(server.address().port, "127.0.0.1");
  let output = "";
  socket.on("data", (chunk) => (output += chunk.toString()));
  socket.on("end", () => {
    assert.match(output, /Transfer-Encoding: chunked/i);
    assert.match(output, /5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n/);
    server.close(() => console.log("http raw chunked response passed"));
  });
  socket.on("connect", () =>
    socket.end("GET /chunked HTTP/1.1\r\nHost: example.com\r\n\r\n")
  );
});
