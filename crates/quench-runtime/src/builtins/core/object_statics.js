// Self-hosted Object static methods over the `__ops__` bridge.
// Prototype manipulation and extension checks are thin 1-line proxies to
// `__ops__`/Rust, so they stay in Rust (see ADR 0001 "aggressive use JS").