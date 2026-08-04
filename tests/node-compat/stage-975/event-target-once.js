const target = new EventTarget();
let calls = 0;
target.addEventListener("ready", () => calls++, { once: true });
target.dispatchEvent(new Event("ready"));
target.dispatchEvent(new Event("ready"));
if (calls !== 1) throw new Error("once listener was invoked more than once");

let repeated = 0;
target.addEventListener("ready", () => repeated++);
target.dispatchEvent(new Event("ready"));
target.dispatchEvent(new Event("ready"));
if (repeated !== 2) throw new Error("ordinary listener did not repeat");
