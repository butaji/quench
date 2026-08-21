const http = require("http");
const net = require("net");
let count = 0;
const server = http.createServer((req, res) => {
  count++;
  res.end("ok");
});
server.httpAllowHalfOpen = true;
server.listen(0, () => {
  const socket = net.createConnection(server.address().port);
  socket.setEncoding("utf8");
  socket.on(
    "connect",
    () => socket.write("GET /a HTTP/1.1\r\nHost: x\r\n\r\n"),
  );
  socket.on("data", () => {
    if (count === 1) {
      socket.write("POST /b HTTP/1.1\r\nHost: x\r\n\r\n");
    } else if (count === 2) {
      socket.write(
        "GET /c HTTP/1.1\r\nHost: x\r\n\r\nGET /d HTTP/1.1\r\nHost: x\r\n\r\n",
      );
      socket.end();
    }
  });
  socket.on("end", () =>
    setTimeout(() => {
      if (count !== 4) throw new Error(`count=${count}`);
      server.close();
    }, 20));
});
