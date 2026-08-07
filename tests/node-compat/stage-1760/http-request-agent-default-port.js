const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => response.end("ok"));
server.listen(0, () => {
  const agent = new http.Agent();
  agent.defaultPort = server.address().port;
  const options = {
    host: undefined,
    hostname: "localhost",
    port: undefined,
    path: undefined,
    method: undefined,
    agent,
  };
  http.request(options, (response) => {
    response.resume().on("end", () => {
      assert.strictEqual(options.port, undefined);
      assert.strictEqual(options.method, undefined);
      server.close();
    });
  }).end();
});
