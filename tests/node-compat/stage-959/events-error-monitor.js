const { EventEmitter, errorMonitor } = require("events");

const emitter = new EventEmitter();
const failure = new Error("monitored error");
let monitored;
let handled;

emitter.on(errorMonitor, (error) => {
  monitored = error;
});
emitter.on("error", (error) => {
  handled = error;
});
emitter.emit("error", failure);

if (monitored !== failure) throw new Error("errorMonitor should see the error");
if (handled !== failure) throw new Error("error listener should still run");
