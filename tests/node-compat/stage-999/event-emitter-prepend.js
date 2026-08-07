const { EventEmitter } = require("events");
const emitter = new EventEmitter();
const order = [];
emitter.on("ready", () => order.push("last"));
emitter.prependListener("ready", () => order.push("first"));
emitter.emit("ready");
if (order.join(",") !== "first,last") {
  throw new Error("prependListener order was incorrect");
}
