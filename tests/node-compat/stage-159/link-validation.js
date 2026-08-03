const fs = require("fs");
for (const value of [false, 1, [], {}, null, undefined]) {
  try {
    fs.linkSync(value, "");
    throw new Error("accepted invalid source");
  } catch (error) {
    if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
  }
  try {
    fs.linkSync("", value);
    throw new Error("accepted invalid target");
  } catch (error) {
    if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
  }
}
