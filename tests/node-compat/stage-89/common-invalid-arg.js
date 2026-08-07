const common = require("../common");

if (!common.invalidArgTypeHelper(false).includes("boolean")) {
  throw new Error("invalidArgTypeHelper missing");
}
