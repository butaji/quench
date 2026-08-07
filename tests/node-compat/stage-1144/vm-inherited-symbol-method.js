const vm = require("vm");

const symbol = Symbol();
function Document() {
  this[symbol] = "foo";
}
Document.prototype.getSymbolValue = function () {
  return this[symbol];
};
const context = vm.createContext(new Document());
if (vm.runInContext("this.getSymbolValue()", context) !== "foo") {
  throw new Error("inherited VM method was not visible");
}
