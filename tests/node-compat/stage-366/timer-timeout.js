const assert = require("assert");
const cancelled = setTimeout(() => order.push("cancelled"), 1);
clearTimeout(cancelled);
let fired = false;
setTimeout(() => {
  fired = true;
}, 1);
queueMicrotask(() => {
  if (!fired) throw new Error("timer did not run");
});
