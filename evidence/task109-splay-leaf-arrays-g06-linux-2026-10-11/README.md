# TOD-109 G-06: Splay leaf-array skipping (Linux)

## Decision

Rejected. The direct profile mark-time reduction was 7.4%, below the 15%
decision bar. Neither 11-pair fixed-work session had a clean hardware-counter
round, the repeat regressed, and pooling both sessions did not produce an
interval excluding zero. Work and setup RSS were slightly higher. The array
classification bitmap and its write-barrier hooks were removed; the production
tree is unchanged by this experiment.

## Provenance

- Source base and control binary: commit `a125dc95f` (`gc: speed Splay mark bit
  path`), binary SHA-256
  `6aaf73b79e0e71419a1ed9ebfe0e705e8c755c72e6ac0f1a4ea9b68e61e27bbd`.
- G-06 candidate binary SHA-256:
  `995cd652d28093d0fc41663c544389d6256d9bd114bf276c8995a4b3aa9cf313`.
- Host: Linux x86_64, 4 logical CPUs, cgroup CPU quota 4 CPUs; Rust
  `1.99.0 (b940084d7 2026-09-28)`.
- Workload: Splay, fixed-work, one marker, 300 iterations, 11 alternating
  control/candidate pairs per session. Both sessions were valid and output
  equal in 11/11 pairs, but had 0/11 clean Linux counter rounds. These wall
  measurements are screening data, not a qualified speed claim.
- The raw session reports and direct-profile logs are regenerable artifacts
  under ignored `work/task109/`; they are not committed.

## Paired fixed-work results

| Session | Median control | Median candidate | Paired median delta | Candidate wins | Bootstrap 95% interval | Work RSS delta | Setup RSS delta |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| First 11 pairs | 3,970,719 ns/run | 3,187,750 ns/run | −168,545 ns (−4.2%) | 7/11 | −1,611,123 to +1,594,941 ns | +200,704 B | +57,344 B |
| Repeat 11 pairs | 2,877,353 ns/run | 3,228,685 ns/run | +210,663 ns | 1/11 | +94,740 to +524,648 ns | +122,880 B | +155,648 B |
| Pooled 22 pairs | — | — | +175,596 ns | 8/22 | −128,237 to +341,881 ns | — | — |

Each session's interval includes zero or shows a regression; the pooled interval
also crosses zero. The second session alone shows a significant regression.

## Diagnostic profile

One unpaired aggregate profile compared the baseline K0/K300 source logs with
the G-06 candidate. The candidate counted 1,124,433 leaf arrays skipped,
400 dense array values scanned, and 8,106,415 work items at K300. Marginal mark
time moved from 0.388714 to 0.359866 ms/run (−7.4%). Sweep moved from 0.250666
to 0.211845 ms/run; total mark plus sweep moved from 0.639380 to 0.571712
ms/run. Because this is one unpaired instrumented profile, the whole-cycle
change is diagnostic only.

## Correctness and removal

While the experiment was active, Rust 1.99 focused heap tests passed 32/32,
VM index tests passed 13/13, warnings-denied runtime check passed, and the
`profile-aggregate` checks passed for the runtime and Node packages. The full
Splay fixed-work comparison remained output-equal. These checks do not qualify
the optimization, and the temporary implementation was reverted after the
paired repeat failed its decision rule.

## Reproduction artifacts

Ignored artifacts used for this decision:

- `work/task109/splay-g06-fixed-11.json`
- `work/task109/splay-g06-fixed-11-repeat.json`
- `work/task109/splay-g06-profile-candidate-k0.stderr`
- `work/task109/splay-g06-profile-candidate.stderr`
- `work/task109/splay-g06-candidate-node`
- `work/task109/splay-baseline-a125-node`

The project fixed-work runner and paired analysis scripts can regenerate the
reports from those binaries. Candidate and control hashes above identify the
executables; the binaries are not retained in git.
