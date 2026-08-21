const timers = require("timers/promises");

(async () => {
  const order = [];
  order.push("start");
  await timers.scheduler.yield();
  order.push("yield");
  await timers.scheduler.wait(1);
  order.push("wait");
  if (order.join(",") !== "start,yield,wait") {
    throw new Error(`unexpected order: ${order.join(",")}`);
  }
  console.log("timers promises scheduler passed");
})();
