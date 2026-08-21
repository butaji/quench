const assert = require("assert");
const http = require("http");
const net = require("net");

const server = http.createServer((request, response) => {
  assert.strictEqual(request.method, "GET");
  assert.strictEqual(request.url, "/raw");
  assert.strictEqual(request.headers.host, "example.com");
  response.writeHead(200, { "Content-Type": "text/plain" });
  response.end("ok");
});

server.listen(0, () => {
  const socket = net.createConnection(server.address().port, "127.0.0.1");
  let output = "";
  socket.on("data", (chunk) => {
    output += chunk.toString();
    if (output.includes("\r\n\r\nok")) {
      assert.match(output, /^HTTP\/1\.1 200 OK/);
      assert.match(output, /Content-Type: text\/plain/);
      socket.destroy();
      server.close(() => console.log("http raw GET roundtrip passed"));
    }
  });
  socket.on("connect", () => {
    socket.write("GET /raw HTTP/1.1\r\nHost: example.com\r\n\r\n");
  });
});
