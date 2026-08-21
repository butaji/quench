const http = require("http");

const server = http.createServer((request, response) => {
  response.setHeader("content-type", "text/plain");
  response.end(`${request.method}:${request.url}`);
});

server.listen(() => {
  const address = server.address();
  fetch(`http://127.0.0.1:${address.port}/hello?x=1`, {
    method: "POST",
    body: "body",
  }).then(async (response) => {
    if (response.status !== 200) throw new Error("wrong status");
    if (response.headers.get("content-type") !== "text/plain") {
      throw new Error("wrong content type");
    }
    if ((await response.text()) !== "POST:/hello?x=1") {
      throw new Error("wrong response body");
    }
    server.close();
    console.log("fetch http bridge passed");
  });
});
