const event = new Event("submit", {
  bubbles: true,
  cancelable: true,
  composed: true,
});

if (event.type !== "submit") throw new Error("Event type was not preserved");
if (!event.bubbles || !event.cancelable || !event.composed) {
  throw new Error("Event flags were not preserved");
}
event.preventDefault();
if (!event.defaultPrevented) {
  throw new Error("Cancelable event was not canceled");
}

const fixed = new Event("load");
fixed.preventDefault();
if (fixed.defaultPrevented) {
  throw new Error("Non-cancelable event was canceled");
}
