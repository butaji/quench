const { REPLServer } = require("repl");
const { EventEmitter } = require("events");

const repl = new REPLServer({ useColors: true });
if (repl.writer.options.colors !== true) {
  throw new Error("REPL writer colors were not enabled");
}

const input = new EventEmitter();
let output = "";
new REPLServer({ input, output: { write: (value) => (output += value) } });
input.emit("data", 'util.inspect("string")\n');
if (!output.includes("\"'string'\"")) {
  throw new Error(`REPL input was not rendered: ${output}`);
}
