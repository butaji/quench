const assert = require("assert");
const http = require("http");

const agent = new http.Agent({
  keepAlive: true,
  maxTotalSockets: 2,
  maxSockets: 3,
});
const servers = [
  http.createServer((req, res) => res.end("one")),
  http.createServer((req, res) => res.end("two")),
];
let responses = 0;
Promise.all(
  servers.map((server) => new Promise((resolve) => server.listen(0, resolve))),
)
  .then(() => {
    for (const server of servers) {
      for (let index = 0; index < 3; index++) {
        http.get(
          { host: "localhost", port: server.address().port, agent },
          (response) => {
            assert.ok(Object.values(agent.sockets).flat().length <= 2);
            response.resume();
            response.once("end", () => {
              responses++;
              if (responses === 6) {
                agent.destroy();
                for (const item of servers) item.close();
                console.log("http agent max total sockets passed");
              }
            });
          },
        );
      }
    }
  });
