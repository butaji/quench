const net = require("net");
let calls = 0;
const server = net.createServer(() => {
  calls++;
});
server.listen(0, () => {
  const client = net.connect(server.address().port);
  client.once("connect", () => client.destroy());
  setTimeout(() => {
    if (calls !== 1) throw new Error(`handler called ${calls} times`);
    server.close();
  }, 20);
});
