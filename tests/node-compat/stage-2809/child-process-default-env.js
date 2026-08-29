const assert = require("assert");
const { spawn } = require("child_process");

process.env.QUENCH_STAGE_ENV = "present";
const child = spawn("/usr/bin/env", [], {});
child.stdout.setEncoding("utf8");
let output = "";
child.stdout.on("data", (chunk) => (output += chunk));
child.on("close", () => assert(output.includes("QUENCH_STAGE_ENV=present")));
