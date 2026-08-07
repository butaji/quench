const zlib = require("zlib");

zlib.gzip("x", { info: true }, (error, result) => {
  if (error) throw error;
  if (!(result.engine instanceof zlib.Gzip)) {
    throw new Error("gzip info engine identity missing");
  }
  console.log("zlib info engine passed");
});
