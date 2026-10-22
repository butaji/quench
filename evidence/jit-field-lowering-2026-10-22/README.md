# General numeric field input extension

## Change

The JS numeric denotation now accepts one guarded cached field read from formal parameter zero in a pure numeric bytecode region, while preserving other numeric parameters and captures in the native ABI. The field operation can appear at any PC in the accepted straight-line region. A cache miss, accessor/proxy, or non-numeric result prevents native entry and leaves the original frame at PC zero for interpreter execution. Existing field-switch and field-reject paths keep their separate entry and ABI rules.

The regression `js_jit_lowers_guarded_numeric_field_reads_into_arithmetic` now exercises `twice(node, scale)` with a numeric second argument, an accessor, and replacement shapes. It also asserts the two-parameter artifact enters native code.

## Correctness evidence

- Host: Linux x86_64 container, 16 GiB memory cgroup.
- Required `quench-runtime --lib` suite passed: 798 passed, 0 failed. Build settings: iteration profile, opt-level 0, 16 codegen units, two Cargo jobs, serialized LLVM backend, debug info disabled to fit the host cgroup.
- `cargo check --locked --profile iteration -p quench-runtime --tests` passed.
- The unit test covers the new multi-parameter numeric ABI and fallback. A broader run of the 116 Node-oracle inventory was not identified or run in this step.

## Diagnostic

The production build from this dirty step ran pinned JetStream 3 `typescript-lib` once with `--jit --jit-diagnostics`. It was valid and output-equal. Diagnostic-mode median scores were Quench 0.12, Bun 1.49, Node 1.64; Quench maximum RSS was 190.7 MiB. This is a one-round run with instrumentation and is not score/RSS qualification evidence.

The diagnostic recorded 712,818 native entries among functions whose opcode census included `GetField`, but that includes existing field-specific compiler paths and does not isolate the new denotation. The new path has functional coverage in the unit test, but no matched before/after score gain is established here.

The report is `typescript-lib-diagnostic.json`. The source base revision is `410d83929`; the production binary SHA-256 is recorded in that report. The change is a functional compiler-extension milestone, not a claim that it meets the JetStream performance target.
