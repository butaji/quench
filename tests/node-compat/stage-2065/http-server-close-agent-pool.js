const assert = require("assert");
const http = require("http");

const server = http.createServer((req, res) => res.end("ok"));
const portPromise = new Promise((resolve) => {
  server.listen(0, () => resolve(server.address().port));
});
portPromise.then((port) => {
  const req = http.get({ port }, (res) => {
    res.resume();
    res.on("end", () => {
      server.close(() => {
        const sockets = Object.values(http.globalAgent.freeSockets).flat();
        assert.strictEqual(
          sockets.some((socket) => socket.__quenchServerPort === port),
          false,
        );
      });
    });
  });
  req.end();
});
