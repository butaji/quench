const vm = require("vm");

const compiled = vm.compileFunction("return p + q;", ["p", "q"]);
if (compiled("a", "b") !== "ab") throw new Error("compiled function failed");
if (
  vm.compileFunction('console.log("Hello, World!")').toString() !==
    'function () {\nconsole.log("Hello, World!")\n}'
) {
  throw new Error("compiled function string form changed");
}
