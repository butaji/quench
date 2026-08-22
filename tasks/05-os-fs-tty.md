# Stage 05 — OS, filesystem, and interactive I/O

Complete `os`, `tty`, `fs`, `fs/promises`, file handles, watchers, symlink/stat/error semantics, and `readline`/promises. Keep VFS and real-provider behavior behind shared declarations; preserve callback, promise, and stream variants without duplicate semantics.

Run upstream fs/os/tty/readline fixtures and focused stages 2360–2429, 2537, 2559, 2592–2593. Acceptance: paths, permissions, offsets, ordering, aborts, watcher lifecycle, and platform-limited behavior are classified explicitly, never skipped silently.
