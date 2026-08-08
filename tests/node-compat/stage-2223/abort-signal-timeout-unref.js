const assert = require("assert");
const { once } = require("events");

const signal = AbortSignal.any([
  AbortSignal.timeout(9000),
  AbortSignal.timeout(110000)
]);
const abortPromise = Promise.race([
  once(signal, "abort").then(([event]) => {
    if (event.type !== "abort") throw new Error("wrong abort event");
    if (signal.reason.name !== "TimeoutError") {
      throw new Error("wrong timeout reason");
    }
  }),
  new Promise((resolve) => setTimeout(resolve, 10000))
]);

assert
  .rejects(() => abortPromise, {
    name: "TimeoutError",
    message: "The operation was aborted due to timeout"
  })
  .then(() => console.log("abort signal timeout unref passed"));
