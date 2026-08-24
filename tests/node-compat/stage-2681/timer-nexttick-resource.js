"use strict";

const events = [];
let cancelled;

setTimeout(() => {
  events.push("timeout");
  process.nextTick(() => {
    events.push("nextTick");
    clearTimeout(cancelled);
  });
}, 1);
cancelled = setTimeout(() => events.push("cancelled"), 1);
setTimeout(() => events.push("final"), 1);

process.on("exit", () => {
  if (events.join(",") !== "timeout,nextTick,final") {
    throw new Error(events.join(","));
  }
});
