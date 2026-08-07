const { EventEmitter } = require("events");

(async () => {
  const emitter = new EventEmitter({ captureRejections: true });
  const failure = new Error("rejected listener");
  let received;
  emitter.on("error", (error) => {
    received = error;
  });
  emitter.on("ready", async () => {
    throw failure;
  });
  emitter.emit("ready");
  await new Promise((resolve) => queueMicrotask(resolve));
  if (received !== failure) {
    throw new Error("captureRejections should emit the original error");
  }
})();
