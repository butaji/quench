const timeout = setTimeout(() => {}, 1000);
if (typeof timeout.refresh !== "function") {
  throw new Error("timeout refresh is missing");
}
if (timeout.refresh() !== timeout) {
  throw new Error("timeout refresh not chainable");
}
clearTimeout(timeout);

const interval = setInterval(() => {}, 1000);
if (typeof interval.refresh !== "function") {
  throw new Error("interval refresh is missing");
}
if (interval.refresh() !== interval) {
  throw new Error("interval refresh not chainable");
}
clearInterval(interval);

console.log("timer handle refresh passed");
