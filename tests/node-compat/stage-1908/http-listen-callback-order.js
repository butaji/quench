const assert = require("assert");
const http = require("http");

const order = [];
const server = http.createServer(() => {});
server.listen(0, () => {
  order.push("callback");
  assert.deepStrictEqual(order, ["after-listen", "callback"]);
  server.close();
});
order.push("after-listen");
