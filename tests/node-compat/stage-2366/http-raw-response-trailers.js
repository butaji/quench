const assert = require("assert");
const http = require("http");
const net = require("net");

const server = http.createServer((request, response) => {
  response.setHeader("Trailer", "X-Check");
  response.addTrailers({ "X-Check": "yes" });
  response.write("body");
  response.end();
});
server.listen(0, () => {
  const socket = net.createConnection(server.address().port, "127.0.0.1");
  let output = "";
  socket.on("data", (chunk) => (output += chunk.toString()));
  socket.on("end", () => {
    assert.match(output, /Trailer: X-Check/i);
    assert.match(output, /4\r\nbody\r\n0\r\nX-Check: yes\r\n\r\n/);
    server.close(() => console.log("http raw response trailers passed"));
  });
  socket.on(
    "connect",
    () => socket.end("GET /trailers HTTP/1.1\r\nHost: example.com\r\n\r\n"),
  );
});
