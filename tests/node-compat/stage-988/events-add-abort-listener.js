const { addAbortListener } = require("events");
const controller = new AbortController();
let calls = 0;
const disposable = addAbortListener(controller.signal, () => calls++);
if (typeof disposable[Symbol.dispose] !== "function") {
  throw new Error("addAbortListener did not return a disposable");
}
controller.abort();
if (calls !== 1) throw new Error("abort listener was not called");

const disposed = new AbortController();
const second = addAbortListener(disposed.signal, () => {
  throw new Error("disposed listener was called");
});
second[Symbol.dispose]();
disposed.abort();
