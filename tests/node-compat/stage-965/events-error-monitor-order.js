const { EventEmitter, errorMonitor } = require("events");

const emitter = new EventEmitter();
const order = [];
const failure = new Error("ordered error");

emitter.on(errorMonitor, (error) => {
  if (error !== failure) throw new Error("monitor received the wrong error");
  order.push("monitor");
});
emitter.on("error", (error) => {
  if (error !== failure) throw new Error("listener received the wrong error");
  order.push("error");
});
emitter.emit("error", failure);

if (order.join(",") !== "monitor,error") {
  throw new Error("errorMonitor should run before error listeners");
}
