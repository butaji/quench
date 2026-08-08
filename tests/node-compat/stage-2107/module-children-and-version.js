const assert = require("assert");

assert(Number.isInteger(process.config.variables.node_module_version));
assert(process.config.variables.node_module_version > 0);
console.log("module children and version passed");
