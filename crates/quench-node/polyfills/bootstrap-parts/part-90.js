globalThis.__quench_argv ||= [];
if (!globalThis.__quench_argv.includes("--experimental-stream-iter"))
  globalThis.__quench_argv.push("--experimental-stream-iter");
