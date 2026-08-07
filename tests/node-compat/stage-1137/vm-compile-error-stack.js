const vm = require("vm");

try {
  vm.compileFunction('throw new Error("boom")', [], { lineOffset: 2 })();
} catch (error) {
  if (!error.stack.startsWith("Error: boom\n    at <anonymous>:3:7")) {
    throw error;
  }
}
