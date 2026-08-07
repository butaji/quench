const timers = require("timers/promises");

(async () => {
  const values = [];
  for await (const value of timers.setInterval(5, "tick")) {
    values.push(value);
    if (values.length === 3) break;
  }
  if (values.join(",") !== "tick,tick,tick") {
    throw new Error("timers/promises setInterval yielded wrong values");
  }
  console.log("timers promises interval passed");
})();
