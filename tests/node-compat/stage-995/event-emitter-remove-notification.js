const { EventEmitter } = require("events");
const emitter = new EventEmitter();
const listener = () => {};
let removed;
emitter.on("removeListener", function (name, callback) {
  if (this !== emitter) {
    throw new Error("removeListener this was not preserved");
  }
  removed = [name, callback];
});
emitter.on("ready", listener);
emitter.removeListener("ready", listener);
if (removed?.[0] !== "ready" || removed[1] !== listener) {
  throw new Error("removeListener notification was incorrect");
}
