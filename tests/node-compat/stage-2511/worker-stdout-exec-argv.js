const assert = require("assert");
const { Worker } = require("worker_threads");

const worker = new Worker(__filename, {
  execArgv: ["--pending-deprecation", "--"],
  stdout: true,
});
let output = "";
worker.stdout.setEncoding("utf8");
worker.stdout.on("data", (chunk) => (output += chunk));
worker.stdout.on("end", () => {
  assert.deepStrictEqual(JSON.parse(output), ["--pending-deprecation"]);
});
