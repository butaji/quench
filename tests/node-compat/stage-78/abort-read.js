const fs = require("fs");
const common = require("../common");

const signal = AbortSignal.abort();
fs.readFile(
  __filename,
  { signal },
  common.mustCall((error) => {
    if (!error || error.name !== "AbortError") {
      throw new Error("expected AbortError");
    }
  }),
);
try {
  fs.readFile(__filename, { signal: "invalid" }, common.mustNotCall());
  throw new Error("invalid signal accepted");
} catch (error) {
  if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
}
