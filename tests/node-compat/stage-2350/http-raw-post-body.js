const assert = require("assert");
const http = require("http");
const net = require("net");

const server = http.createServer((request, response) => {
  let body = "";
  request.on("data", (chunk) => (body += chunk.toString()));
  request.on("end", () => {
    assert.strictEqual(request.method, "POST");
    assert.strictEqual(request.headers["content-length"], "5");
    assert.strictEqual(body, "hello");
    response.end("ok");
  });
});

server.listen(0, () => {
  const socket = net.createConnection(server.address().port, "127.0.0.1");
  let output = "";
  socket.on("data", (chunk) => {
    output += chunk.toString();
    if (output.endsWith("ok")) {
      assert.match(output, /^HTTP\/1\.1 200 OK/);
      socket.destroy();
      server.close(() => console.log("http raw POST body passed"));
    }
  });
  socket.on("connect", () => {
    socket.write(
      "POST /body HTTP/1.1\r\nHost: example.com\r\nContent-Length: 5\r\n\r\nhello"
    );
  });
});
