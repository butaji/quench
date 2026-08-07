const { Writable } = require("stream");
const writable = new Writable();
let callbackCalled = false;
writable.destroy();
writable.destroy(new Error("ignored"), (error) => {
  if (error !== undefined) throw new Error("repeat destroy received an error");
  callbackCalled = true;
});
queueMicrotask(() => {
  if (!callbackCalled) {
    throw new Error("repeat destroy callback was not called");
  }
}, 0);
