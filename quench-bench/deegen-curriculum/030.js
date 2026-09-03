// Small event-queue state-machine simulation
// stage=full-system-closure; mechanism=A generic task-queue simulation (structurally similar to classic VM benchmarks, but sized and shaped as a neutral general program, not a fixture-detecting kernel).
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function Task(id, priority) { this.id = id; this.priority = priority; this.done = false; }
  function runQueue(tasks) {
    let processed = 0;
    const queue = tasks.slice().sort((a, b) => a.priority - b.priority);
    while (queue.length) {
      const task = queue.shift();
      task.done = true;
      processed++;
      if (task.priority > 0 && processed % 5 === 0) queue.push(new Task(1000 + processed, task.priority - 1));
    }
    return processed;
  }
  const tasks = Array.from({ length: 60 }, (_, i) => new Task(i, i % 4));
  return runQueue(tasks);
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
