const { EventEmitterAsyncResource } = require("events");

const emitter = new EventEmitterAsyncResource();
if (emitter.emit("missing")) {
  throw new Error("emit should return false without listeners");
}
emitter.on("ready", () => {});
if (!emitter.emit("ready")) {
  throw new Error("emit should return true with a listener");
}
