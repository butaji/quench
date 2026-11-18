// Evaluates a script that only defines functions (ops.js), so a lane-plan trace
// (QUENCH_TRACE=bytecode=lanes) sees every function compiled and none run.
"use strict";
(0, eval)(require("fs").readFileSync(process.argv[2], "utf8"));
