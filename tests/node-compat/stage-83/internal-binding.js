const { internalBinding } = require("internal/test/binding");

if (typeof internalBinding !== "function") {
  throw new Error("internalBinding facade missing");
}
if (
  !internalBinding("fs") || typeof internalBinding("fs").fstat !== "function"
) {
  throw new Error("fs binding facade missing");
}
