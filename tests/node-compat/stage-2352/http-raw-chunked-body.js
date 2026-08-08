const assert = require("assert");
const http = require("http");
const net = require("net");

const server = http.createServer((request, response) => {
  let body = "";
  request.on("data", (chunk) => (body += chunk.toString()));
  request.on("end", () => response.end(body));
});
server.listen(0, () => {
  const socket = net.createConnection(server.address().port, "127.0.0.1");
  let output = "";
  socket.on("data", (chunk) => (output += chunk.toString()));
  socket.on("end", () => {
    assert.match(output, /HTTP\/1\.1 200 OK/);
    assert.match(output, /hello world/);
    server.close(() => console.log("http raw chunked body passed"));
  });
  socket.on("connect", () => {
    socket.end(
      "POST /chunked HTTP/1.1\r\nHost: example.com\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n"
    );
  });
});
