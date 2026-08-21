const assert = require("assert");
const http = require("http");
const common = require("../../../tests/node/common");
const Countdown = require("../../../tests/node/common/countdown");

const maxSockets = 2;
const agent = new http.Agent({
  keepAlive: true,
  keepAliveMsecs: 1000,
  maxSockets,
  maxFreeSockets: 2,
});
let serverCalls = 0;
const server = http.createServer(
  common.mustCall((_request, response) => {
    serverCalls++;
    response.end("hello world");
  }, 6),
);
const countdown = new Countdown(6, () => server.close());

server.listen(
  0,
  common.mustCall(() => {
    for (let index = 0; index < 6; index++) {
      const request = http.get(
        { host: "localhost", port: server.address().port, agent, path: "/1" },
        common.mustCall(() => {}),
      );
      request.on(
        "response",
        common.mustCall(() => {
          request.abort();
          const sockets = agent.sockets[Object.keys(agent.sockets)[0]] || [];
          assert.ok(sockets.length <= maxSockets);
          countdown.dec();
        }),
      );
    }
  }),
);

process.on("exit", () => {
  assert.strictEqual(serverCalls, 6);
});
