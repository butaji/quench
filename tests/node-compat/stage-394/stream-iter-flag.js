const enabled = globalThis.__quench_argv.includes("--experimental-stream-iter");
let error;
try {
  const iter = require("node:stream/iter");
  if (!enabled || typeof iter.text !== "function") {
    throw new Error("stream/iter enabled unexpectedly");
  }
} catch (caught) {
  error = caught;
}

if (enabled && error) throw error;
if (!enabled && (!error || error.code !== "ERR_UNKNOWN_BUILTIN_MODULE")) {
  throw new Error("stream/iter must be gated by its experimental flag");
}

console.log(`stream iter flag ${enabled ? "enabled" : "disabled"} passed`);
