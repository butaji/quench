const assert = require("assert");
const http = require("http");

let socketsCreated = 0;
class Agent extends http.Agent {
  createConnection(options, callback) {
    socketsCreated++;
    return super.createConnection(options, callback);
  }
}

const server = http.createServer((_request, response) => response.end());
server.listen(0, () => {
  const agent = new Agent({ keepAlive: true, maxSockets: 1 });
  const request = http.get(
    { agent, port: server.address().port },
    (response) => {
      response.resume();
      response.once("end", () => {
        response.destroy();
        http.get({ agent, port: server.address().port }, (second) => {
          second.resume();
          assert.strictEqual(socketsCreated, 1);
          agent.destroy();
          server.close();
        });
      });
    },
  );
  request.end();
});
