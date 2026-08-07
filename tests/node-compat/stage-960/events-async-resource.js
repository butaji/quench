const { EventEmitterAsyncResource } = require("events");

const emitter = new EventEmitterAsyncResource({ name: "stage-960" });
let received;
emitter.on("ready", (value) => {
  received = value;
});
emitter.emit("ready", 42);

if (received !== 42) {
  throw new Error("async resource emitter should emit events");
}
if (!emitter.asyncResource) {
  throw new Error("async resource emitter should expose its resource");
}
if (emitter.asyncResource.type !== "stage-960") {
  throw new Error("async resource should preserve its name");
}
