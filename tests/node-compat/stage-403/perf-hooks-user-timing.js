const { performance } = require("perf_hooks");

performance.clearMarks();
const start = performance.mark("stage-start");
const end = performance.mark("stage-end");
if (start.entryType !== "mark" || end.entryType !== "mark") {
  throw new Error("performance.mark returned an invalid entry");
}
const measure = performance.measure("stage", "stage-start", "stage-end");
if (measure.entryType !== "measure" || measure.duration < 0) {
  throw new Error("performance.measure returned an invalid entry");
}
performance.clearMarks("stage-start");

console.log("perf hooks user timing passed");
