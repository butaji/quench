const target = new NodeEventTarget();
let receivedThis;
target.addEventListener("ready", function (event) {
  receivedThis = this;
  if (event.type !== "ready") throw new Error("event type was not preserved");
});
target.dispatchEvent(new Event("ready"));
if (receivedThis !== target) {
  throw new Error("function listener this was not preserved");
}
