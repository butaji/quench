const assert = require("assert");
const http = require("http");
const net = require("net");

const agent = new http.Agent({ keepAlive: true });
const socket = new net.Socket();
socket._handle = { ref() {}, readStart() {} };
const server = http.createServer((request, response) => response.end());

server.listen(0, () => {
  const request = new http.ClientRequest(
    `http://localhost:${server.address().port}/`,
  );
  agent.freeSockets[agent.getName(request)] = [socket];
  agent.addRequest(request, {});
  request.on("response", () => {
    assert.strictEqual(request.reusedSocket, true);
    agent.destroy();
    server.close();
    console.log("http agent partial handle passed");
  });
  request.end();
});
