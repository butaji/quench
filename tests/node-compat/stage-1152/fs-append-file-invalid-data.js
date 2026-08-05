const fs = require("fs");

for (const data of [false, 5, {}, null, undefined]) {
  const filename = `append-invalid-${Date.now()}-${typeof data}.txt`;
  try {
    fs.appendFileSync(filename, data);
    throw new Error("invalid append data was accepted");
  } catch (error) {
    if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
  }
}
