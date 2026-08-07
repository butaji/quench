const { EventEmitter } = require("events");

(async () => {
  const previous = EventEmitter.captureRejections;
  EventEmitter.captureRejections = true;
  const emitter = new EventEmitter();
  const failure = new Error("default rejection");
  let received;
  emitter.on("error", (error) => {
    received = error;
  });
  emitter.on("ready", async () => {
    throw failure;
  });
  emitter.emit("ready");
  await new Promise((resolve) => queueMicrotask(resolve));
  EventEmitter.captureRejections = previous;
  if (received !== failure) {
    throw new Error("instances should inherit static captureRejections");
  }
})();
