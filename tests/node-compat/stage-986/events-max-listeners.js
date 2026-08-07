const { getMaxListeners, setMaxListeners } = require("events");
const target = new EventTarget();
const before = getMaxListeners(target);
setMaxListeners(3, target);
if (getMaxListeners(target) !== 3) {
  throw new Error("listener limit was not set");
}
setMaxListeners(before, target);
