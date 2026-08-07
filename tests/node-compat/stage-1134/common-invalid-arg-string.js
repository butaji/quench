const common = require("../common");

if (
  common.invalidArgTypeHelper("string") !== " Received type string ('string')"
) {
  throw new Error("string argument formatting is not Node-shaped");
}
