const util = require("util");
const error = new Error("format failure");
if (util.format(error) !== error.stack) throw new Error(util.format(error));
