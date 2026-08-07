const { timerify, PerformanceObserver } = require("perf_hooks");

const observer = new PerformanceObserver(() => {
  throw new Error("observer should not receive thrown function entries");
});
observer.observe({ entryTypes: ["function"] });
const wrapped = timerify(() => {
  throw new Error("timerified failure");
});
let error;
try {
  wrapped();
} catch (caught) {
  error = caught;
}
if (!error || error.message !== "timerified failure") {
  throw new Error("timerify did not preserve thrown errors");
}
observer.disconnect();

console.log("perf hooks timerify passed");
