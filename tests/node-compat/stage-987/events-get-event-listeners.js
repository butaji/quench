const { getEventListeners } = require("events");
const target = new EventTarget();
const first = () => {};
const second = () => {};
target.addEventListener("ready", first);
target.addEventListener("ready", first);
target.addEventListener("ready", second);
const listeners = getEventListeners(target, "ready");
if (
  listeners.length !== 2 || listeners[0] !== first || listeners[1] !== second
) {
  throw new Error("EventTarget listener identities were not normalized");
}
