const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  response.setHeader("content-type", "application/json");
  response.end(JSON.stringify({ path: request.url }));
});

server.listen(40124, () => {
  const request = http.get("http://localhost:40124/test", (response) => {
    response.setEncoding("utf8");
    let body = "";
    response.on("data", (chunk) => (body += chunk));
    response.on("end", () => {
      assert.deepStrictEqual(JSON.parse(body), { path: "/test" });
      server.close();
    });
  });
  request.unref();
});
