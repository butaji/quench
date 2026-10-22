# Compiling eval origins and reusable script identity

Frozen source 725f91d335c1b13568b9497183d208389e502046 passes 368/368 serial comparisons against stable Node v26.12.0 (exit zero, 228.44 seconds), 656 runtime tests, workspace/all-target checking, eight focused eval comparisons, seven source matrices, 43 framework fixtures and nine no-JIT web correctness targets. CLI, runner and test hashes stay fixed through the full API run. Source patches and two added fixture hashes reconcile to the commit. Later Function-constructor naming changes are excluded.

The integrated excerpt repair 85b2cb408 shares JavaScript line boundaries with compiler positions, preserving CRLF, CR, LF, Unicode separators, UTF16 wrapper columns and trailing empty lines. Negative controls print wrapper text instead of the actual failing line; the five-terminator Node excerpt comparison and source-layout unit test pass.

Eval origins snapshot the actual compiling caller's immutable source name and position without invoking name hooks. Nested origins and retained CallSites survive collection. Unnamed eval scripts expose absent CallSite filenames and empty detailed script names; empty debug names use the eval display label. Both ordinary and all four Function-constructor origins are covered, without claiming complete constructor-body positions/default debug names.

The first origin matrix locates direct eval five columns into its argument. A temporary trace shows a wide call lacks its own source anchor after rewriting. Relocation now inspects the logical decoded opcode, preserving call anchors through later argument-Move removal; a two-pass regression and expanded spread/parameter/optional/alias tests pass. Lexical-binding ownership remains separate. Temporary tracing is removed before qualification; its source patch, provenance and raw output remain retained.

Node reuses indirect-eval script identity across contexts. The program store reuses a live compiled program, preserving its first origin and script-ID relation while execution supplies the target realm and fresh functions, objects and class prototypes. Strict scripts and parser-owned tagged-template syntax, including nested sites, are excluded according to the inspected V8 cache rules. Entries validate program identity and disappear through the existing reclamation/reset lifecycle. An initial assertion expecting separate origins across realms fails on Node itself; corrected controls retain shared origins and target-realm values. Exact V8 cache ageing and contextual direct-eval identity remain open.

A full runtime build exhausts disk before tests. Incremental test caches are removed and retries disable incremental caching. An exec-server reset interrupts a later runtime/workspace pair with no terminal results; those logs are not successful tests. Final retries use CARGO_INCREMENTAL=0 and two build jobs, then pass all 656 runtime tests and workspace checking before the isolated full API run. Initial string-builder type failures are retained separately.

All 32 strict matched instruction captures are valid with unchanged frozen identities against 03ea582d2. Each binary supplies its own startup subtraction; regular outputs must complete the exact fixture count. These are integrated instruction comparisons, not isolated attribution, throughput, RSS or benchmark dominance.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
| crypto | +0.5632% | pass |
| deltablue | -34.2974% | pass |
| earley-boyer | +1.6046% | FAIL |
| navier-stokes | -0.9794% | pass |
| raytrace | -11.9831% | pass |
| regexp | -6.1169% | pass |
| richards | -31.2621% | pass |
| splay | +0.9217% | pass |

Startup increases by 36,779,182–245,662,723 instructions. No authored optimization, total Node parity or web benchmark win is claimed. SourceURL/source maps, complete stack APIs, constructor default names/positions, internal-key visibility and broader Node conformance remain open. The retained constructor probe shows native constructor display still reports anonymous where Node reports Object; that repair is subsequent work.
