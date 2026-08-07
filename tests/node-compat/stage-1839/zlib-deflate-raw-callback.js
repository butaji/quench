const zlib = require("zlib");

zlib.deflateRaw("raw", (error, compressed) => {
  if (error) throw error;
  zlib.inflateRaw(compressed, (error2, result) => {
    if (error2) throw error2;
    if (result.toString() !== "raw") {
      throw new Error("raw callback roundtrip failed");
    }
    console.log("zlib raw callbacks passed");
  });
});
