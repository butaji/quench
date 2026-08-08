const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  assert.strictEqual(request.url, "/ping");
  response.end("pong");
});
server.listen(0, () => {
  const request = http.get(
    `http://127.0.0.1:${server.address().port}/ping`,
    (response) => {
      let body = "";
      response.on("data", (chunk) => (body += chunk));
      response.on("end", () => {
        assert.strictEqual(body, "pong");
        server.close(() => console.log("http request response passed"));
      });
    }
  );
  request.on("error", (error) => {
    throw error;
  });
});
