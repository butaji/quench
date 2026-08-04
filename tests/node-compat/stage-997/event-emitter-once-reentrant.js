const { EventEmitter } = require("events");
const emitter = new EventEmitter();
let calls = 0;
emitter.once("ready", () => {
  calls++;
  emitter.emit("ready");
});
emitter.once("ready", () => calls++);
emitter.emit("ready");
if (calls !== 2) throw new Error("re-entrant once listeners were incorrect");
