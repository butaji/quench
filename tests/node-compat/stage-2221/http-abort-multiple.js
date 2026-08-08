const assert = require("assert");
const http = require("http");

const total = 8;
let aborted = 0;
const server = new http.Server((request, response) => {
  request.on("aborted", () => {
    aborted += 1;
    assert.strictEqual(request.aborted, true);
    if (aborted === total) server.close();
  });
  response.write("working");
});

server.listen(0, () => {
  let responses = 0;
  const requests = [];
  for (let index = 0; index < total; index += 1) {
    const request = http.get({ port: server.address().port }, (response) => {
      response.resume();
      responses += 1;
      if (responses === total) {
        // The requests are intentionally retained by this closure until both
        // response callbacks have been delivered.
        requests.forEach((request) => request.abort());
      }
    });
    requests.push(request);
  }
});
