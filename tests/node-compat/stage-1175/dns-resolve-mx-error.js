const common = require("../../node/test/common");
const dns = require("dns");

dns.resolveMx(
  "foo.onion",
  common.mustCall((error) => {
    if (error.code !== "ENOTFOUND" || error.syscall !== "queryMx") {
      throw new Error("unexpected resolveMx error");
    }
  }),
);
