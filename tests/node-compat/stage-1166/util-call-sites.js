const util = require("util");

if (typeof util.getCallSites !== "function") {
  throw new Error("util.getCallSites is unavailable");
}
if (!Array.isArray(util.getCallSites())) {
  throw new Error("util.getCallSites did not return an array");
}
