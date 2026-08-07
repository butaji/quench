const repl = require("repl");
const server = repl.start({ prompt: "" });
if (typeof server.write !== "function") throw new Error("REPL write missing");
server.write("(async () => {})()\n");
server.write(".exit\n");
if (!server.closed) throw new Error("REPL did not close");
console.log("repl write exit passed");
