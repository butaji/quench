const assert = require("assert");
const { Duplex } = require("stream");

const duplex = Duplex.from({
  readable: new ReadableStream({
    start(controller) {
      controller.enqueue("foo");
      controller.close();
    }
  })
});
assert.strictEqual(duplex.readable, true);
duplex.on("data", (data) => {
  assert.strictEqual(data.toString(), "foo");
  console.log("stream duplex nested web readable pass");
});
