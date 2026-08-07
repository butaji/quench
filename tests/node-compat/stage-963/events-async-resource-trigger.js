const { EventEmitterAsyncResource } = require("events");

const emitter = new EventEmitterAsyncResource({
  name: "stage-963",
  triggerAsyncId: 37,
});

if (emitter.asyncResource.triggerAsyncId() !== 37) {
  throw new Error("async resource should preserve triggerAsyncId");
}
