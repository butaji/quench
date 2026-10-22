# Structured-clone and filesystem parity repairs — 2026-10-15

The qualified production sources are `9095ca60e` (clone semantics) and
`3e503bc16` (application-relative filesystem paths), based on shared source
`5858b2949`. Executable identities are in `qualification.json`. Node v26.12.0
is the official stable oracle; its release source archive is hash checked.
Subsequent transfer-marker changes have separate qualification and must not
inherit the counts in this directory.

## Behavior repaired

The shared VM clones native Errors, Map, Set, Date, RegExp, all twelve typed
arrays, DataView and boxed primitives. It preserves graph aliases, cycles,
internal values, intrinsic prototypes, resize tracking and ordinary buffer
transfer semantics. Error descriptor snapshots precede getters; message strings
retain UTF-16 code units. Map/Set snapshots survive deletion during a getter.
Private builtin data bypasses public constructors, insertion methods and
iterators. Promises, arguments objects and boxed symbols reject cloning.

The Node boundary uses the pinned Node Web IDL converter for options, errors,
iteration and getter order. The VM accepts the converted transfer sequence
without consulting a public iterator again. The Node library is guest code
executed by Quench, with Rust owning the graph and host effects.

Filesystem paths now follow the application's working directory. Production
code no longer rewrites `test/...` into this repository's Node fixtures or
bypasses the current directory for `tests/node/test/...`. The paired real-CLI
counterexample initially prints `[true,true]` on Node and `[false,false]` on
Quench; the repaired executable prints `[true,true]`. Raw input/output and a
regression covering both relative prefixes are retained.

## Qualification

- Six clone differential tests pass against official Node, including getter
  mutation, allocation/GC pressure, UTF-16, typed views and invalid options.
- The filesystem working-directory differential passes.
- The full API suite passes **258/258 serially**, with no failed reruns hidden.
- Shared runtime library passes **619/619**; workspace/all-target check passes.
- Framework-core passes **43/43**.
- All **nine** pinned framework/runtime combinations pass correctness, including
  the full streamed video checksum. Node uses `--jitless`, Bun reports
  `useJIT=false`, and Quench runs interpreted. This is verification-only.

Both all-eight instruction guards are complete and within the 1% per-run
limit. The clone candidate ranges from -0.0393% to +0.0118%; the filesystem
candidate ranges from -0.0217% to +0.0238%. The latter refresh reuses only the
baseline counts from the identical frozen executable, exact input paths and
same host; all sixteen candidate counts are fresh. Raw counts, input hashes
and normalized results are retained. These are iteration-build instructions
and include shared-lane changes, so they do not isolate attribution or prove
production throughput/RSS. Earlier performance findings use other baselines
and remain tracked separately in TOD-135.

## Remaining work

Total Node parity remains open, including shared-memory aliasing, host-object
cloning, transferable streams/ports, workers and other Node APIs. Later work
repairs transfer markers and serialization/commit ordering; those changes need
their own checks. High-concurrency web socket timeouts remain unscored.
Optimization stays behind the user's parity requirement, and no benchmark win
is claimed from this milestone. Linear: TOD-133, TOD-134, TOD-135.
