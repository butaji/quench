const assert = require("assert");
const zlib = require("zlib");

assert(zlib.Gzip() instanceof zlib.Gzip);
assert(new zlib.Gzip() instanceof zlib.Gzip);
