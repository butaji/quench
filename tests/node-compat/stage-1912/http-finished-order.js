const http = require("http");
const { finished } = require("stream");

const order = [];
const server = http.createServer((request, response) => {
  let closed = false;
  response.on("close", () => {
    closed = true;
    order.push("response-close");
    finished(response, () => order.push("finished-after-close"));
  });
  response.end();
  finished(response, () => {
    order.push(`finished-before-close:${closed}`);
    console.log(order.join(","));
    server.close();
  });
});
server.listen(0, () => {
  http.get({ port: server.address().port }, (response) => {
    response.resume();
  });
});
