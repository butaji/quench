const { performance } = require("perf_hooks");

performance.clearMarks();
performance.clearMeasures();
performance.mark("entry-start");
performance.measure("entry-measure", "entry-start");

if (performance.getEntriesByName("entry-start", "mark").length !== 1) {
  throw new Error("getEntriesByName did not find the mark");
}
if (performance.getEntriesByType("measure").length !== 1) {
  throw new Error("getEntriesByType did not find the measure");
}
if (performance.getEntries().length !== 2) {
  throw new Error("getEntries returned the wrong number of entries");
}

performance.clearMeasures("entry-measure");
if (performance.getEntriesByType("measure").length !== 0) {
  throw new Error("clearMeasures did not remove the measure");
}

console.log("perf hooks entries passed");
