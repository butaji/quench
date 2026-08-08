const assert = require("assert");
const http = require("http");

const CUSTOM_TIMEOUT = 60;
const AGENT_TIMEOUT = 50;
class CustomAgent extends http.Agent {
  keepSocketAlive(socket) {
    assert.strictEqual(super.keepSocketAlive(socket), true);
    socket.setTimeout(CUSTOM_TIMEOUT);
    return true;
  }
}

const agent = new CustomAgent({ keepAlive: true, timeout: AGENT_TIMEOUT });
const server = http.createServer((request, response) => response.end());
server.listen(0, () => {
  http.get({ port: server.address().port, agent }, (response) => {
    const socket = response.socket;
    response.resume();
    socket.once("free", () => {
      assert.strictEqual(socket.timeout, CUSTOM_TIMEOUT);
      socket.once("timeout", () => {
        assert.strictEqual(socket.timeout, CUSTOM_TIMEOUT);
        agent.destroy();
        server.close();
        console.log("http agent custom timeout passed");
      });
    });
  });
});
