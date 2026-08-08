const assert = require("assert");
const http = require("http");

const agent = new http.Agent({ keepAlive: true });
const server = http.createServer((request, response) => response.end());
server.listen(0, () => {
  http.get({ port: server.address().port, agent }, (response) => {
    const socket = response.socket;
    assert.ok(socket);
    response.resume();
    socket.once("free", () => {
      agent.destroy();
      server.close();
      console.log("http agent free event passed");
    });
  });
});
