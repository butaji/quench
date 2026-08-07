const timers = require("timers/promises");

const start = Date.now();
timers.setTimeout(15, "done").then((value) => {
  if (value !== "done") throw new Error("timer promise lost its value");
  if (Date.now() - start < 10) {
    throw new Error("timers/promises setTimeout ignored its delay");
  }
  console.log("timers promises delay passed");
});
