const util = require("util");
if (!util.types.isDate(new Date())) throw new Error("util.types regression");
