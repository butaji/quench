const target = new NodeEventTarget();
let calls = 0;
const listener = () => calls++;

if (target.listenerCount("ready") !== 0) throw new Error("count was not empty");
if (target.on("ready", listener) !== target) {
  throw new Error("on was not chainable");
}
if (target.listenerCount("ready") !== 1) {
  throw new Error("listener was not counted");
}
if (!target.eventNames().includes("ready")) {
  throw new Error("event name was missing");
}
target.dispatchEvent(new Event("ready"));
if (calls !== 1) throw new Error("listener was not dispatched");
if (target.off("ready", listener) !== target) {
  throw new Error("off was not chainable");
}
if (target.listenerCount("ready") !== 0) {
  throw new Error("listener was not removed");
}
