if (typeof fetch !== "function") throw new Error("fetch global is unavailable");
if (Object.keys(globalThis).includes("__quench_fs_read_file")) {
  throw new Error("host bindings leaked as enumerable globals");
}
