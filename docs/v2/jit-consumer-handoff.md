# Local JIT consumer handoff

## Ready review target

`v2-jit-foundation-release-20261027` is the clean, local review branch. Its base is
`da9a1c41b71aabd259d6ab848f06e819ae8946b7`; implementation source is the tested
`72c770283d1a7646e2e643df39d2f51041a2fb17`. It adds the reusable extension guide
and runnable captured-leaf example. The later diagnostic experiment
`d918ffbfd24798638f796d9f88c74f749c49162b` is excluded.

## Build and smoke

```sh
source /workspace/quench-build-env.sh
CARGO_TARGET_DIR=target/jit-consumer CARGO_INCREMENTAL=0 \
  cargo build --offline --locked --profile production -j2 \
  -p quench-node -p quench-test262 --bin quench-node --bin run-test
node examples/jit/captured-leaf.cjs
target/jit-consumer/production/quench-node --no-jit examples/jit/captured-leaf.cjs
target/jit-consumer/production/quench-node --jit examples/jit/captured-leaf.cjs
```

Each smoke run prints `[160000,18000]`. For native-body proof, run JIT in a
separate diagnostic process with `QUENCH_JS_JIT_STATS=1`; find `capturedLeafLoop`
and require `native_region_runs > exits + 20`. Do not use diagnostic timings.

Tested on source `72c770283d`: 1,038 runtime tests; 53,353 Test262 passes and
53 unchanged waivers per mode with zero unwaived failures; 67,124 Wasm directives
passed per mode across 288 files; and 16 V8-v7 functional checks. Cold AES,
TypeScript and Babel parity also passed. This is correctness and mechanism evidence,
not score/RSS qualification. Exact binary, corpus and report hashes are in
`/workspace/quench-jit-evidence-20261025/captured-inline/consumer-packet.json`.

## Consumer source relationships

**Zod.** The locally available base is `e5d2d2795ca4059a8eae9095f5c9642bca2133ce`.
Git reports that as the merge base with this runtime, so it predates the JIT changes.
The existing candidate supports a restricted captured leaf call: stable captured
identity, exact arity, receiver-independent eligible body, existing activation and
recovery checks. It does not support the locale `getTypeName` helper (own capture,
global reads, nested calls, varying arity) or make acyclic field-call `operations()`
native. The local Zod 4.6.7 extraction passed Node/JIT/no-JIT semantic parity;
no package speed claim is made. Consumer run commands: build above, then the smoke
example; validate its actual package entry points and report native coverage
separately before performance testing.

**JetStream.** The consumer owner supplied this ancestry metadata:
`fb9861cf990254a34d1813db9f7ca10ccef985a` has tree
`6004766d7a6501f9d53eb012f399f57c0f32e425`, parent
`6134751ccc840ac7ac5af1b73af932764e128d96`, and merge parents
`2f98092b16c7fcc30baf996fc4e803a1f8306e0b` and
`56b9ee8fcee4f0191358b659e5fd74c9897da039`, whose reported merge base is
`d0e1f471f8a911ba318c48213e8cd9a7ceae9bb1`. The owner reports three Cloud Wasm
commits (`5fd031692c97321d2dcc75e030aac205c7ed29f8`,
`d152bf4dc79e2894df00a14e431dc64a555a7624`, `2f98092`) and an OSR array
receiver guard/harness fix in `fb9861cf`. Those Git objects are unavailable in
this checkout; ancestry and conflict-free integration are therefore unverified.
The supplied graph places the JIT candidate beside the two merge parents; it does
not establish that it is already included. First fetch through the authorized
repository workflow, verify every object and the merge base, then inspect a trial
merge in an isolated worktree. Resolve only demonstrated conflicts and rerun the
consumer's stock correctness/native smoke before any JetStream timing. No patch or
source has been transferred here.

**Fuzzilli.** Consumer owner reports build commit `cebb58b5` fails with 42
warnings-as-errors in legacy shared native/JIT paths. Expected foundation artifact
contract: use the common JS `Entry` and existing shared target ISA/`CodeMemory`
publication, code-cache budget, tier diagnostics, and interpreter fallback. Build
from the consumer's pinned source and canonical locked command, separately for
no-JIT and JIT; record toolchain, features, profile/PGO training identity, flags,
source hash and binary hash. Run the fixed generated corpus with the harness and
host configuration pinned, retaining exact crashes, mismatches and compiler
warnings. Pass findings to the source owner for targeted fixes. Do not weaken
`-D warnings`, suppress warnings, or import this branch until source objects and
integration are authorized and verified. No unverified consumer-specific feature
set or PGO training hash is asserted by this packet.

## Remaining boundary

Consumers currently lack source access because this branch is local and publication
approval remains pending. No consumer integration failure is established from
these reports. Consumer workloads own their package/native coverage and performance
qualification. General rooted effectful-call continuation, dynamic instruction
count A/A, peer score/RSS qualification, and publication/adoption remain open.
