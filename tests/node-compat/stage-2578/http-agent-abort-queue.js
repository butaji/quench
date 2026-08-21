const assert = require("assert");
const http = require("http");

const agent = new http.Agent({ keepAlive: true, maxSockets: 2 });
const server = http.createServer((req, res) => res.end("ok"));
let responses = 0;
server.listen(0, () => {
  for (let index = 0; index < 6; index++) {
    const request = http.get({
      host: "localhost",
      port: server.address().port,
      agent,
      path: "/1",
    }, () => {
      responses++;
      request.abort();
      const name = Object.keys(agent.sockets)[0];
      assert.ok(name);
      assert.ok(agent.sockets[name].length <= 2);
      if (responses === 6) {
        agent.destroy();
        server.close();
        console.log("http agent abort queue passed");
      }
    });
    request.on("error", assert.fail);
  }
});
