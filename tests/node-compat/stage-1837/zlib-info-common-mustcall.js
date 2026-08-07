const common = require("../common");
const zlib = require("zlib");
const expect = "blah".repeat(8);
zlib.gzip(
  expect,
  { info: true },
  common.mustCall((error, result) => {
    if (error) throw error;
    if (!(result.engine instanceof zlib.Gzip)) {
      throw new Error("Gzip identity missing with mustCall");
    }
    console.log("zlib info common mustCall passed");
  }),
);
