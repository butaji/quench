const allowedBindings = [
  "buffer",
  "cares_wrap",
  "constants",
  "contextify",
  "fs",
  "fs_event_wrap",
  "icu",
  "inspector",
  "js_stream",
  "natives",
  "os",
  "pipe_wrap",
  "spawn_sync",
  "stream_wrap",
  "tcp_wrap",
  "tls_wrap",
  "tty_wrap",
  "udp_wrap",
  "util",
  "uv",
  "zlib",
];
for (const name of allowedBindings) {
  if (!process.binding(name)) throw new Error(`missing binding: ${name}`);
}
try {
  process.binding("not-a-real-binding");
  throw new Error("unknown binding did not throw");
} catch (error) {
  if (error.code !== "ERR_UNKNOWN_BUILTIN_MODULE") throw error;
}
