const target = new NodeEventTarget();
const listener = {
  handleEvent(event) {
    if (this !== listener) throw new Error("listener this was not preserved");
    if (event.type !== "ready") throw new Error("event type was not preserved");
  },
};

target.addEventListener("ready", listener, { once: true });
target.dispatchEvent(new Event("ready"));
if (target.listenerCount("ready") !== 0) {
  throw new Error("once object listener was not removed");
}
