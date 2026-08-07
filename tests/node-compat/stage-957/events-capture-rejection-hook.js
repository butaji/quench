const { EventEmitter } = require("events");

(async () => {
  const emitter = new EventEmitter({ captureRejections: true });
  const failure = new Error("hooked rejection");
  let received;
  emitter[Symbol.for("nodejs.rejection")] = (error, event) => {
    received = { error, event };
  };
  emitter.on("ready", async () => {
    throw failure;
  });
  emitter.emit("ready");
  await new Promise((resolve) => queueMicrotask(resolve));
  if (!received || received.error !== failure || received.event !== "ready") {
    throw new Error("capture rejection hook should receive error and event");
  }
})();
