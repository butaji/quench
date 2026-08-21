const assert = require("assert");
const http = require("http");
const net = require("net");

let handled = 0;
const server = http.createServer((request, response) => {
  handled++;
  response.end(`${request.trailers["x-check"] || "missing"}:${request.url}`);
});
server.listen(0, () => {
  const socket = net.createConnection(server.address().port, "127.0.0.1");
  let output = "";
  socket.on("data", (chunk) => (output += chunk.toString()));
  socket.on("end", () => {
    assert.strictEqual(handled, 2);
    assert.match(output, /yes:\/first/);
    assert.match(output, /missing:\/second/);
    server.close(() => console.log("http raw trailers keepalive passed"));
  });
  socket.on("connect", () => {
    socket.end(
      "POST /first HTTP/1.1\r\nHost: example.com\r\nTransfer-Encoding: chunked\r\n\r\n1\r\na\r\n0\r\nX-Check: yes\r\n\r\nGET /second HTTP/1.1\r\nHost: example.com\r\n\r\n",
    );
  });
});
