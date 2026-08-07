const { EventEmitterAsyncResource } = require("events");

const emitter = new EventEmitterAsyncResource({ name: "stage-961" });
let observed;
emitter.on("ready", () => {
  observed = emitter.asyncResource.asyncId();
});
emitter.emit("ready");

if (!Number.isInteger(observed) || observed < 1) {
  throw new Error("event emission should run with a valid async resource id");
}
if (emitter.asyncResource.asyncId() !== observed) {
  throw new Error("async resource id should remain stable");
}
