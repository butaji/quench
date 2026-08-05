const { REPLServer } = require("repl");

const repl = new REPLServer({ useColors: true });
if (repl.writer.options.colors !== true)
  throw new Error("REPL writer colors were not enabled");
