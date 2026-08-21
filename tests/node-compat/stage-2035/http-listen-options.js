const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => response.end("ok"));
server.listen({ port: 0, host: "127.0.0.1" }, () => {
  assert.strictEqual(server.listening, true);
  assert.strictEqual(server.address().address, "127.0.0.1");

  http.get(`http://localhost:${server.address().port}`, (response) => {
    let body = "";
    response.setEncoding("utf8");
    response.on("data", (chunk) => (body += chunk));
    response.on("end", () => {
      assert.strictEqual(body, "ok");
      server.close();
    });
  });
});

console.log("http listen options passed");
