const { utf8Write } = require("internal/buffer");
if (typeof utf8Write !== "function") {
  throw new Error("internal utf8Write missing");
}
