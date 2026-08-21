const { once } = require("events");

const signal = AbortSignal.any([
  AbortSignal.timeout(10),
  AbortSignal.timeout(100),
]);
setTimeout(() => {}, 30);
once(signal, "abort").then(([event]) => {
  if (event.type !== "abort") throw new Error("wrong abort event");
  if (signal.reason.name !== "TimeoutError") {
    throw new Error("wrong timeout reason");
  }
  console.log("abort signal timeout passed");
});
