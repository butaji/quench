const { performance } = require("perf_hooks");

if (typeof performance.now() !== "number") {
  throw new Error("performance.now must return a number");
}
if (typeof performance.timeOrigin !== "number") {
  throw new Error("performance.timeOrigin must return a number");
}
const before = performance.now();
if (performance.now() < before) {
  throw new Error("performance.now moved backward");
}
if (typeof performance.toJSON !== "function") {
  throw new Error("performance.toJSON must exist");
}

console.log("perf hooks performance passed");
