const target = new EventTarget();
let passivePrevented = false;
target.addEventListener(
  "submit",
  (event) => {
    event.preventDefault();
    passivePrevented = event.defaultPrevented;
  },
  { passive: true },
);
const passiveEvent = new Event("submit", { cancelable: true });
if (target.dispatchEvent(passiveEvent) !== true) {
  throw new Error("Passive dispatch was reported as canceled");
}
if (passivePrevented) throw new Error("Passive listener canceled the event");

const activeEvent = new Event("submit", { cancelable: true });
target.addEventListener("submit", (event) => event.preventDefault());
if (target.dispatchEvent(activeEvent) !== false) {
  throw new Error("Active listener did not cancel the event");
}
