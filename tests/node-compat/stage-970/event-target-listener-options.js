const target = new EventTarget();
const controller = new AbortController();
let calls = 0;
const listener = () => calls++;

target.addEventListener("message", listener, {
  once: true,
  signal: controller.signal,
});
target.dispatchEvent(new Event("message"));
target.dispatchEvent(new Event("message"));
if (calls !== 1) throw new Error("once listener was called more than once");

const removed = () => {
  throw new Error("aborted listener was called");
};
target.addEventListener("message", removed, { signal: controller.signal });
controller.abort();
target.dispatchEvent(new Event("message"));
