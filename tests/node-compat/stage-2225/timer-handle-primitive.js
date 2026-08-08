const timeout = setTimeout(() => {
  throw new Error("cleared timeout fired");
}, 100);
const timeoutId = +timeout;
if (!Number.isInteger(timeoutId) || timeoutId <= 0) {
  throw new Error("timeout handle is not a positive integer");
}
clearTimeout(timeoutId);
const stringTimeout = setTimeout(() => {
  throw new Error("cleared string timeout fired");
}, 100);
clearTimeout(`${stringTimeout}`);

const interval = setInterval(() => {
  throw new Error("cleared interval fired");
}, 100);
const intervalId = +interval;
if (!Number.isInteger(intervalId) || intervalId <= timeoutId) {
  throw new Error("interval handle is not a unique integer");
}
clearInterval(intervalId);

setTimeout(() => console.log("timer handle primitive passed"), 0);
