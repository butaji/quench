const { EventEmitterAsyncResource } = require("events");

const emitter = new EventEmitterAsyncResource();

if (emitter.asyncResource.type !== "EventEmitterAsyncResource") {
  throw new Error("async resource should have the default name");
}
