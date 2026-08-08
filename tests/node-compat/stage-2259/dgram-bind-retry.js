const assert = require("assert");
const dgram = require("dgram");

const reserved = dgram.createSocket("udp4");
reserved.bind(() => {
  const port = reserved.address().port;
  const retry = dgram.createSocket("udp4");
  let errors = 0;
  retry.on("error", (error) => {
    assert.strictEqual(error.code, "EADDRINUSE");
    errors++;
    if (errors === 3) {
      retry.close();
      reserved.close();
      console.log("dgram bind retry passed");
    } else {
      retry.bind(port);
    }
  });
  retry.bind(port);
});
