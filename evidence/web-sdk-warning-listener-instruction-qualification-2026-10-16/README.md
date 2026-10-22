# Frozen SDK warning qualification

Source b9745ab5b203d0974f17c18f1b5f7bb77a9ae0b6 passes 406 serial Node v26.12.0 API comparisons, 43 framework fixtures and nine no-JIT web correctness targets. Source/CLI/runner/test/oracle manifests match before and after the passing API run. Runtime/RegExp/Core Cargo inputs match the integrated 672-test passing a45551524 tree. Later source is excluded.

All 32 instruction captures are valid with unchanged identities. Frozen CLI hashes and source manifests identify the measured code even if the profiler records a later working-tree revision. The baseline is frozen 03ea582d2. Candidate iteration opt1/256 CGUs/no LTO/incremental=false, integrated shared-source differences and concurrent host activity prevent isolated source attribution, production timing or RSS claims.

| Fixture | Instruction delta | Guard |
| --- | ---: | --- |
| crypto | -3.5262% | pass |
| deltablue | -38.1574% | pass |
| earley-boyer | -4.5789% | pass |
| navier-stokes | -6.5118% | pass |
| raytrace | -17.6128% | pass |
| regexp | -4.3381% | pass |
| richards | -39.2073% | pass |
| splay | -9.3227% | pass |

SDK warning listener policy removes default output when listeners are removed and preserves suppression/startup options. The newer sdk-emit-warning source also uses the actual SDK public emitWarning function, avoiding extra Proxy reads and preserving public source/identity/constructor policy; promise producers use ordinary public string/type or Error arguments. Its focused and original upstream fixtures are retained. Initial direct upstream-file runs treated CommonJS fixtures as ESM because of the surrounding repository package type; correct run-parallel --one runs pass. These setup failures are not parity findings. The newer qualification also retains its initial 407/408 API run (Quench SIGTERM handler did not return status7 under the existing 300ms deadline), unchanged focused pass, paired 20/20 ready-marker/handler probe and unchanged full repeat. The transient cause remains unproven; no deadline changed. The older warning-listener qualification separately retains its initial Node-oracle PID startup deadline failure and unchanged passing repeat; no deadlines changed.

Diagnostic report generation, native metrics, abort/async hooks, broader source/namespace behavior and total Node parity remain open. The latest short Express JSON load has 249 timeouts. There is no framework throughput win or authored performance optimization.
