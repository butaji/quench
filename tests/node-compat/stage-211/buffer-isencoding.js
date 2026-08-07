const { Buffer } = require("buffer");

for (const encoding of ["utf8", "UTF-8", "hex", "base64url", "ucs2"]) {
  if (!Buffer.isEncoding(encoding)) {
    throw new Error(`encoding rejected: ${encoding}`);
  }
}
for (const encoding of ["utf9", 1, null, {}, undefined]) {
  if (Buffer.isEncoding(encoding)) throw new Error("invalid encoding accepted");
}
