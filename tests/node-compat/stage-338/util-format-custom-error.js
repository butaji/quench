const util = require("util");
function BadCustomError(message) {
  Error.call(this);
  Object.defineProperty(this, "message", { value: message });
  Object.defineProperty(this, "name", { value: "BadCustomError" });
}
Object.setPrototypeOf(BadCustomError.prototype, Error.prototype);
Object.setPrototypeOf(BadCustomError, Error);
if (util.format(new BadCustomError("foo")) !== "[BadCustomError: foo]") {
  throw new Error(util.format(new BadCustomError("foo")));
}
