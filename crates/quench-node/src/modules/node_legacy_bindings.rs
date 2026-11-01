// Node's legacy `process.binding` lists, from `lib/internal/bootstrap/realm.js` of the
// pinned Node source set (`node/SOURCE`).
const PROCESS_BINDING_ALLOW_LIST: &[&str] = &[
    "buffer",
    "cares_wrap",
    "config",
    "constants",
    "contextify",
    "fs",
    "fs_event_wrap",
    "icu",
    "inspector",
    "js_stream",
    "os",
    "pipe_wrap",
    "process_wrap",
    "spawn_sync",
    "stream_wrap",
    "tcp_wrap",
    "tls_wrap",
    "tty_wrap",
    "udp_wrap",
    "uv",
    "zlib",
];
const RUNTIME_DEPRECATED_BINDINGS: &[&str] = &[
];
const LEGACY_WRAPPER_BINDINGS: &[&str] = &[
    "natives",
    "util",
];
