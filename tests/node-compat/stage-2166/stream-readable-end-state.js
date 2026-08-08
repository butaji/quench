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
duplex.on("end", () => {
  assert.strictEqual(duplex.readable, false);
  console.log("stream readable end state pass");
});
duplex.resume();
