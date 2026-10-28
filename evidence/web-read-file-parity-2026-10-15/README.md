# Read-file parity and strict instruction evidence

Candidate `a49952862` passed 269 serial API differentials against Node 26.12.0, 623 runtime tests, 43 framework checks and all nine no-JIT web correctness targets. These are partial functional checks, not total Node parity or web throughput results.

The strict verifier `b94452437` checks exit status, exact fixed-work completion marker, guest stderr, binary/input hashes before and after, and positive instrumented Irefs. All 32 paired captures are valid. DeltaBlue (+5.701%) and Earley-Boyer (+1.655%) exceed the 1% regression limit; the performance gate fails. This integrated comparison does not isolate attribution or measure throughput/RSS. Baseline 03ea582d is freshly captured with the same regular-file output method; older piped baseline counts must not be reused here.

`frozen-guard-qualified-replay/report.json` records 144 valid native executions across nine frozen historical binaries and sixteen identical inputs. The negative controls deliberately reject missing markers, nonzero process exit despite a marker, and positive Irefs without completion. They are verifier tests, never benchmark candidates.

The application filesystem utility filename collision is retained as a counterexample against this frozen candidate; its subsequent repair is 4cc515065 and is not included in this candidate's results. Later supplied-buffer and options repairs also require their own qualification.
