const target = new NodeEventTarget();
let eventType;
let rawValue;
target.addEventListener("data", (event) => {
  eventType = event.type;
});
target.on("data", (value) => {
  rawValue = value;
});

if (!target.emit("data", "payload")) {
  throw new Error("emit did not report delivery");
}
if (eventType !== "data") {
  throw new Error("EventTarget listener did not receive Event");
}
if (rawValue !== "payload") {
  throw new Error("Node listener did not receive argument");
}
