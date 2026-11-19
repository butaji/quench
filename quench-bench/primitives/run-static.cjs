// Runs one op (argv: name n) through the primitives module linked as a build-time unit, so
// `quench-gate --only aot-primitives` measures the code a produced image ships for it.
"use strict";
require("./ops.js");
const name = process.argv[2];
const f = globalThis.PRIMITIVE_OPS[name];
process.stdout.write(f ? String(f(1000)) : "unknown");
