const util = require("util");
for (const options of [undefined, null, false, 5, "test"]) {
  let error;
  try {
    util.formatWithOptions(options, { a: true });
  } catch (caught) {
    error = caught;
  }
  if (!error || error.code !== "ERR_INVALID_ARG_TYPE") {
    throw new Error(String(error));
  }
}
