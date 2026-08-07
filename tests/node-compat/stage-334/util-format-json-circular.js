const util = require("util");
const value = {};
value.self = value;
if (util.format("%j", value) !== "[Circular]") {
  throw new Error(util.format("%j", value));
}
