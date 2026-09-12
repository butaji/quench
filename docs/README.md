# Documentation

- [Rules and Lisp mindset manifesto](../AGENTS.md)
- [Active task queue](../tasks/index.json) and [task execution contract](../tasks/README.md)
  (critical, profiled, host and deferred lanes)
- [Runtime architecture](architecture.md)
- [Copy-and-patch JIT contract](stencil-jit-implementation-spec.md)
- [Native generation reference](native-generation.md)
- [Evidence records](architecture-evidence.md) and [mechanism probes](native-micro-curriculum.md)
- [Execution-profile contracts](execution-contract-tests.md)
- [Performance protocol](performance-lanes.md), [V8-v7 commands](v8_v7.md), and [benchmark independence](benchmark-integrity.md)
- [Micros manual](../quench-bench/micros/README.md)
- [Wasm boundary](spec.md)
- [Test262 stages](STAGES.md) and [Node stages](../STAGES.md): runner-consumed data
- [Native execution core](../crates/quench-runtime/src/native_core/PROVENANCE.md)

Run `node tools/check-task-coherence.mjs` after queue or documentation edits. It
checks task headings against the queue, supported statuses, dependency/lane
references, the next critical-path item, required task sections and local
Markdown links, plus the shared evidence vocabulary below. It also verifies
that the single execution-profile corpus still contains exactly 342 schema-3
`optimized` records with the required `ir` array, so the prose cannot drift
from the JSON authority.

Documentation describes contracts and verified implementation, not progress diaries.
Task state lives only in the queue. Completed work and superseded narratives are
available in Git history. Measurement artifacts belong under ignored `target/`
directories; paths alone do not establish source or binary provenance.
Task files own current gaps, scope and definitions of done; architecture and JIT
specification pages describe the shared contracts those tasks extend.

## Shared vocabulary

These three evidence levels are intentionally separate:

1. **Canonical IR contract:** the 342 execution-profile JSON files describe the
   best verified hot-IR target for the current lowering policy. A green run proves
   that target and its result contracts, not globally optimal IR or machine code.
2. **Native execution evidence:** focused runtime tests prove artifact validation,
   native entry, guards, exits and exact fallback. A generated opcode or selected
   artifact is not evidence of entry by itself.
3. **Production performance evidence:** matched V8-v7/Bun/JSC/Node runs measure
   throughput and costs on the target machine. A score never rewrites the IR
   contract, and an IR pass never implies a speedup.

“Complete” in a task therefore means complete for that task’s owned authority and
consumers. It does not mean universal native opcode coverage or fastest-VM status;
those are later qualification claims with their own evidence. The current queue
is a finite sequence of authority transitions (a later profile may add a new
bounded item), while optimization capacity has no arbitrary architectural
ceiling: only documented resource budgets may select a safe canonical fallback.

The roadmap has one source for each kind of fact: `tasks/index.json` owns status
and dependencies; task files own bounded work and definitions of done; the
execution-profile contract owns the 342 JSON/IR expectation; runtime tests own
semantic and native-entry evidence; and the performance protocol owns V8-v7,
Bun/JSC and Node measurements. Do not copy a fact into a second authority or
regenerate JSON expectations from current output without an explicit lowering
contract change.

The native execution core is compiled directly under `quench-runtime`; builds
do not read sibling repositories. Core replacement remains task 098 and is not
complete until direct opcode/region generation and zero-failure gates land. The complete Test262 gate is
deterministic and uncapped:

```sh
TEST262_REPORT=target/test262-report.json \
  cargo run --release -p quench-test262 --bin run-all
```

Every discovered runnable file must be accounted for and pass. Unsupported
syntax, harness errors, host gaps, timeouts and crashes are failures, not
implicit skips.

For long runs, execute deterministic file batches sequentially (the batch
indices cover the sorted discovery list without relying on stage metadata):

```sh
for i in $(seq 0 53); do
  TEST262_BATCH_SIZE=1000 TEST262_BATCH_INDEX="$i" \
    TEST262_REPORT="target/test262-batches/file-${i}.json" \
    target/release/run-all || exit $?
done
```
