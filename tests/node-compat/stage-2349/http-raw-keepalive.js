const assert = require("assert");
const http = require("http");
const net = require("net");

let requests = 0;
const server = http.createServer((request, response) => {
  requests++;
  response.end(String(requests));
});
server.listen(0, () => {
  const socket = net.createConnection(server.address().port, "127.0.0.1");
  let output = "";
  socket.on("data", (chunk) => {
    output += chunk.toString();
    if (output.endsWith("2")) {
      assert.strictEqual(requests, 2);
      socket.destroy();
      server.close(() => console.log("http raw keepalive passed"));
    }
  });
  socket.on("connect", () => {
    socket.write(
      "GET /one HTTP/1.1\r\nHost: example.com\r\n\r\n" +
        "GET /two HTTP/1.1\r\nHost: example.com\r\n\r\n",
    );
  });
});
