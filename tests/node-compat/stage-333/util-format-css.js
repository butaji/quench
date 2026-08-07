const util = require("util");
if (util.format("%cab", "color: blue") !== "ab") {
  throw new Error(util.format("%cab", "color: blue"));
}
if (util.format("%cab", "color: blue", "c") !== "ab c") {
  throw new Error(util.format("%cab", "color: blue", "c"));
}
