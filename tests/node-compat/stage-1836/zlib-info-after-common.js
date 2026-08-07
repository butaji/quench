require("../common");
const zlib = require("zlib");

zlib.gzip("info", { info: true }, (error, result) => {
  if (error) throw error;
  if (!(result.engine instanceof zlib.Gzip)) {
    throw new Error("Gzip identity changed after common load");
  }
  console.log("zlib info after common passed");
});
