const assert = require("assert");
const http = require("http");

const expected = {
  DELETE: ["host", "connection"],
  GET: ["host", "connection"],
  HEAD: ["host", "connection"],
  OPTIONS: ["host", "connection"],
  POST: ["host", "connection", "content-length"],
  PUT: ["host", "connection", "content-length"],
  TRACE: ["host", "connection"],
};
const server = http.createServer((request, response) => {
  assert.deepStrictEqual(
    Object.keys(request.headers).sort(),
    expected[request.method].sort(),
  );
  response.end();
});

server.listen(0, () => {
  Promise.all(
    Object.keys(expected).map(
      (method) =>
        new Promise((resolve) => {
          const request = http.request({ method, port: server.address().port });
          request.once("response", resolve);
          request.end();
        }),
    ),
  ).then(() => server.close());
});

console.log("http client default headers passed");
