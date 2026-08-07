const { EventEmitterAsyncResource } = require("events");

const emitter = new EventEmitterAsyncResource({ name: "stage-962" });
if (emitter.emitDestroy() !== emitter) {
  throw new Error("emitDestroy should be chainable");
}
