const assert = require("assert");
const { Duplex } = require("stream");

const duplex = Duplex.from({
  readable: new ReadableStream({
    start(controller) {
      controller.enqueue("value");
      controller.close();
    },
  }),
});
duplex.on("end", () => assert.strictEqual(duplex.readable, false));
duplex.resume();
