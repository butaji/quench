const vm = require("vm");

globalThis.harnessValue = 5;
vm.runInNewContext("harnessValue = 2");
if (globalThis.harnessValue !== 5) {
  throw new Error("new context leaked a host assignment");
}
delete globalThis.harnessValue;
