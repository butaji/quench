const assert = require("assert");
const http = require("http");

const events = [];
let requests = 0;
const server = http.createServer((request, response) => {
  events.push(`request:${request.url}`);
  requests++;
  response.setHeader("connection", "keep-alive");
  response.end(`body-${requests}`);
});

server.listen(0, "127.0.0.1", () => {
  const port = server.address().port;
  const fetch = (path) =>
    new Promise((resolve, reject) => {
      const request = http.get({ host: "127.0.0.1", port, path }, (response) => {
        let body = "";
        response.on("data", (chunk) => (body += chunk));
        response.on("end", () => {
          events.push(`end:${path}`);
          resolve(body);
        });
      });
      request.on("error", reject);
    });

  fetch("/first")
    .then((first) => fetch("/second").then((second) => [first, second]))
    .then(([first, second]) => {
      assert.strictEqual(first, "body-1");
      assert.strictEqual(second, "body-2");
      assert.deepStrictEqual(events.filter((event) => event.startsWith("request:")), [
        "request:/first",
        "request:/second",
      ]);
      assert.deepStrictEqual(events.filter((event) => event.startsWith("end:")), [
        "end:/first",
        "end:/second",
      ]);
      server.close(() => console.log("http sequencing boundary passed"));
    })
    .catch((error) => {
      server.close(() => process.nextTick(() => { throw error; }));
    });
});
