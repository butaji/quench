const assert = require("assert");
const http = require("http");

let handled = 0;
const server = http.createServer((request, response) => {
  handled++;
  response.end(`response-${handled}`);
});
server.listen(0, () => {
  const port = server.address().port;
  const fetch = (path) =>
    new Promise((resolve, reject) => {
      const request = http.get(
        `http://127.0.0.1:${port}${path}`,
        (response) => {
          let body = "";
          response.on("data", (chunk) => (body += chunk));
          response.on("end", () => resolve(body));
        },
      );
      request.on("error", reject);
    });
  Promise.all([fetch("/one"), fetch("/two")]).then((values) => {
    assert.deepStrictEqual(values.sort(), ["response-1", "response-2"]);
    server.close(() => console.log("http two requests passed"));
  });
});
