const assert = require("assert");
const http = require("http");

assert.throws(() => new http.Agent({ scheduling: "filo" }), {
  code: "ERR_INVALID_ARG_VALUE",
  name: "TypeError",
});

const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
assert.strictEqual(agent.scheduling, "lifo");
assert.strictEqual(new http.Agent({ scheduling: "fifo" }).scheduling, "fifo");

const server = http.createServer((req, res) => res.end("ok"));
server.listen(0, () => {
  const request = http.get(
    {
      host: "localhost",
      port: server.address().port,
      agent,
    },
    (response) => {
      const name = Object.keys(agent.sockets)[0];
      assert.ok(name);
      assert.strictEqual(agent.sockets[name].length, 1);
      response.resume();
      response.once("end", () => {
        agent.destroy();
        server.close();
        console.log("http agent scheduling validation passed");
      });
    },
  );
  request.on("error", assert.fail);
});
