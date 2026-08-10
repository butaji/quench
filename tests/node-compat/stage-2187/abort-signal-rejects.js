const assert = require("assert");
const { once } = require("events");

const signal = AbortSignal.any([
  AbortSignal.timeout(10),
  AbortSignal.timeout(100),
]);
const race = Promise.race([
  once(signal, "abort").then(() => {
    throw signal.reason;
  }),
  new Promise((resolve) => setTimeout(resolve, 30)),
]);

assert
  .rejects(() => race, {
    name: "TimeoutError",
    message: "The operation was aborted due to timeout",
  })
  .then(() => console.log("abort signal rejects passed"));
