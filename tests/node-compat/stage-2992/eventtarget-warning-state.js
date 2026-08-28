const assert = require("assert");
const { setMaxListeners } = require("events");

const warnings = [];
process.on("warning", (warning) => warnings.push(warning.message));

const target = new EventTarget();
setMaxListeners(2, target);
for (let index = 0; index < 3; index++) {
  target.addEventListener("foo", () => {});
}

setMaxListeners(2);
const port = new MessageChannel().port1;
for (let index = 0; index < 3; index++) {
  port.addEventListener("foo", () => {});
}

const signal = new AbortController().signal;
setMaxListeners(1, signal);
for (let index = 0; index < 2; index++) {
  signal.addEventListener("foo", () => {});
}

setTimeout(() => {
  assert.deepStrictEqual(
    warnings,
    [
      "Possible EventTarget memory leak detected. 3 foo listeners added to EventTarget. MaxListeners is 2. Use events.setMaxListeners() to increase limit",
      "Possible EventTarget memory leak detected. 3 foo listeners added to [MessagePort [EventTarget]]. MaxListeners is 2. Use events.setMaxListeners() to increase limit",
      "Possible EventTarget memory leak detected. 2 foo listeners added to [AbortSignal]. MaxListeners is 1. Use events.setMaxListeners() to increase limit",
    ],
  );
}, 0);
