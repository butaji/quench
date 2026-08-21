# wasi

Quench currently exposes `args`, `env`, `preopens`, `wasiImport`, `start()`,
and `initialize()` as a compatibility surface. Bun's current matrix supports
the first five except that its documented gaps include `getImportObject()`,
`initialize()`, and `sock_accept`; it also ignores `version`, `returnOnExit`,
and stdio options. Quench MUST record which behavior is real versus a
compatibility stub and validate it with focused and applicable upstream Node
API tests. The native WASI syscall backend remains unsupported.
