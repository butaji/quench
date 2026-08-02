const assert = require("assert");
const error = new SyntaxError("custom error");
try {
  assert(false, error);
} catch (caught) {
  if (caught !== error) throw caught;
}
