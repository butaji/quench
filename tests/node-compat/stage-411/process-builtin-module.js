const path = process.getBuiltinModule("path");
if (!path || typeof path.join !== "function") {
  throw new Error("getBuiltinModule did not return path");
}
if (process.getBuiltinModule("not-a-built-in") !== undefined) {
  throw new Error("unknown getBuiltinModule name must be undefined");
}

console.log("process builtin module passed");
