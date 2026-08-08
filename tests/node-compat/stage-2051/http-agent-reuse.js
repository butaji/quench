const assert = require("assert");
const http = require("http");

const agent = new http.Agent({ keepAlive: true });
const server = http.createServer((request, response) => response.end());
server.listen(0, () => {
  let socket;
  http.get({ port: server.address().port, agent }, (response) => {
    socket = response.socket;
    response.resume();
    socket.once("free", () => {
      http.get({ port: server.address().port, agent }, (next) => {
        assert.strictEqual(socket, next.socket);
        agent.destroy();
        server.close();
        console.log("http agent reuse passed");
      });
    });
  });
});
