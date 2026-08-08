const assert = require("assert");
const dgram = require("dgram");

const options = { type: "udp4", reusePort: true };
const first = dgram.createSocket(options);
const second = dgram.createSocket(options);
first.bind(0, () => {
  const port = first.address().port;
  second.bind(port, () => {
    assert.strictEqual(second.address().port, port);
    first.close();
    second.close();
    console.log("dgram reuse port passed");
  });
});
