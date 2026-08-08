# Host (Rust) surface — the unsafe / OS-bound primitives

## Contract alignment

This task supports the Node 24 application-runtime contract on Linux x86_64;
observable Node behavior remains the compatibility target. Rust remains limited
to engine integration and unsafe or OS-bound primitives.
See `docs/authoritative-test-sources.md` for the Node, LLRT, Deno, WPT, and
Test262 reference roles.

## Goal

`crates/quench-node/src/main.rs` (currently 466 lines) provides host
callbacks as `globalThis.__quench_*` names. The principle is: keep the
Rust host minimal, push Node behaviour into JS polyfills. The host
exposes only what cannot be done safely in pure JS:

- File descriptors and `O_RDONLY`/`O_WRONLY` etc. via `libc`.
- Signals (`SIGTERM`, `SIGINT`, `SIGKILL`, `SIGHUP`, `SIGUSR1`,
  `SIGUSR2`, `SIGPIPE`, `SIGWINCH`).
- Real time (CLOCK_REALTIME, CLOCK_MONOTONIC).
- User identity (`getuid`, `geteuid`, `getgid`, `getegid`).
- Network (`socket`, `bind`, `connect`, `accept`, `sendto`, `recvfrom`,
  `poll`, `setsockopt`, `getsockname`).
- DNS resolution (`getaddrinfo`, `getnameinfo`).
- Process spawn (`fork`+`execve`, `posix_spawn`).
- Process metadata (`sysctlbyname`, `uname`).
- Memory statistics (`task_info` on macOS, `/proc/self/status` on Linux).

## Existing surface

`crates/quench-node/src/main.rs` already exposes:

- `__quench_fs_*` (28 callbacks): exists, mkdir, read, write, readdir,
  rename, unlink, copy, append, access, realpath, chmod, symlink,
  link, readlink, open, kind, openSync, appendSync, writeBytes, readBytes,
  readRange, readRangeBytes, writeHex, readHex, truncate, removeDir,
  mkdtemp.
- `__quench_env_*`: get / set / delete / keys.
- `__quench_process_*`: pid, ppid, getuid, geteuid, getgid, getegid,
  exec_path, argv, homedir, hostname, tmpdir, cpu_count, cwd,
  chdir, platform, arch, umask, now_ns, sleep_ms.
- `__quench_random_*`: uuid, bytes.
- `__quench_sha256`, `__quench_sha256_bytes`.
- `__quench_console_write`.

## Backlog

The remaining host work, in dependency order. Each row is a slice.

### Cluster / child process

- `__quench_script_source` — string. One line: store the entry source
  before evaluating it. Used by the cluster polyfill's `fork()` to
  re-evaluate the entry in worker mode.
- `__quench_process_spawn` — argv + env + cwd → child pid + stdio
  fds. Use `posix_spawn` on macOS, `fork+execve` on Linux.
- `__quench_process_kill` — pid + signal. Maps Node signals to libc.
- `__quench_process_wait` — pid → exit code or signal.
- `__quench_process_pipe` — opens a pipe fd pair; returned to JS as a
  buffer/stream.
- `__quench_socketpair` — `socketpair(AF_UNIX, SOCK_STREAM, 0)`.
- `__quench_ipc_send` / `__quench_ipc_recv` — writev/readv on a Unix
  domain socket with the Node IPC framing (handshake + JSON or
  advanced serialization header).

### Networking

- `__quench_tcp_connect` — host + port → fd.
- `__quench_tcp_bind` — host + port + backlog → listening fd.
- `__quench_tcp_accept` — listening fd → client fd + peer address.
- `__quench_socket_read` / `__quench_socket_write` — fd + buffer → count.
- `__quench_socket_close` — fd.
- `__quench_socket_shutdown` — fd + how (read/write/both).
- `__quench_socket_getsockname` / `getpeername`.
- `__quench_socket_setopt` — `SO_REUSEADDR`, `SO_KEEPALIVE`, `TCP_NODELAY`,
  `SO_RCVBUF`, `SO_SNDBUF`.
- `__quench_udp_socket` / `__quench_udp_send` / `__quench_udp_recv` /
  `__quench_udp_close`.
- `__quench_dns_lookup` — host → address. Use `getaddrinfo`.
- `__quench_dns_reverse` — address → hostname. Use `getnameinfo`.

### TLS

- `__quench_tls_connect` — host + port + ALPN + SNI + cert → fd.
- `__quench_tls_accept` — fd + cert → fd.
- `__quench_tls_handshake` — fd → result.

This requires `openssl-sys` or `rustls` in the dependency tree. Stage 520
registers the JS surface with an explicit unsupported error; defer this host
boundary until the TCP socket slices are available so TLS can reuse their
descriptor and lifecycle contracts.

### TTY

- `__quench_tty_isatty` — fd → bool. `isatty(3)`.
- `__quench_tty_get_winsize` — fd → rows, cols. `ioctl(TIOCGWINSZ)`.
- `__quench_tty_set_raw` — fd → mode flags. `ioctl(TCSETS)`.

Stage 521 covers the JS surface and confirms the simulator reports ordinary
file descriptors as non-TTYs. The host callbacks remain required for real
terminal detection, window sizing, and raw-mode transitions.

### Misc

- `__quench_signal_listen` — signal → callback. Use `signal_hook`.
- `__quench_signal_unlisten`.
- `__quench_memory_rss` / `__quench_memory_heap` / `__quench_memory_external`.
  macOS: `task_info(TASK_VM_INFO)`. Linux: `/proc/self/status`.
- `__quench_uname` — `uname(2)`.
- `__quench_loadavg` — `getloadavg(3)`.

### Process and child exit

- `__quench_exit` — code. Calls `std::process::exit(code)`. Used by
  `process.exit()` when it should actually exit the harness.
- `__quench_kill` — pid + signal. Real kill via `kill(2)`.

## Slicing rules

- One host callback per slice.
- The slice is "done" when the focused stage that uses the callback
  passes AND the matching up-stream fixture in
  `tests/node/test/parallel/<prefix>-*.js` runs without throwing
  (under `tools/run-node-tests.sh`).
- The Rust callback contract is documented in
  `tasks/015-tooling.md` (host-callback contract template).

## Done when

- All `__quench_*` names that JS code references are implemented.
- The Rust file is still under 1 500 lines (target: the host stays
  minimal).

## Status

In progress. The `__quench_script_source` callback and the cluster
host family are the next batch. Network and TLS are sequenced after
the cluster slice.
