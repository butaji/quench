const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  assert.strictEqual(response.writeHead(200), response);
  response.end("abc");
});

server.listen(0, () => {
  http.get({ port: server.address().port }, (response) => {
    const chunks = [];
    response.on("data", (chunk) => chunks.push(chunk));
    response.on("end", () => {
      assert(Buffer.isBuffer(chunks[0]));
      assert.strictEqual(Buffer.concat(chunks).toString(), "abc");
      server.close();
    });
    response.resume();
  });
});

console.log("http response buffer chunks ok");
