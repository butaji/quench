const vm = require("vm");

globalThis.callbackValue = 1;
function updateCallbackValue() {
  globalThis.callbackValue = 2;
}

vm.runInNewContext("callback()", { callback: updateCallbackValue });
if (globalThis.callbackValue !== 2) {
  throw new Error("host callback mutation was not preserved");
}
if (globalThis.callback !== undefined) {
  throw new Error("temporary callback binding leaked");
}

delete globalThis.callbackValue;
