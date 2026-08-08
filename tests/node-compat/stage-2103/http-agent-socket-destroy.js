const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  response.end("ok");
});

server.listen(0, () => {
  const agent = new http.Agent({ maxSockets: 1 });
  const request = http.get(
    { host: "localhost", port: server.address().port, agent },
    (response) => {
      response.resume();
      response.on("end", () => {
        assert.strictEqual(typeof request.socket.destroy, "function");
        request.socket.destroy();
        assert.strictEqual(request.socket.destroyed, true);
        server.close();
        agent.destroy();
      });
    }
  );
});
