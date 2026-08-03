const fs = require("fs");

try {
  fs.closeSync("fd");
  throw new Error("invalid fd accepted");
} catch (error) {
  if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
}
try {
  fs.close(1);
  throw new Error("missing callback accepted");
} catch (error) {
  if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
}
