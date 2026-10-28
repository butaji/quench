# QuickSort root-first policy screen

These three-round JetStream 3 measurements screen the existing bounded
root-first Wasm compilation policy on `quicksort-wasm`. The default policy
report and an explicit root-9 report are retained separately. Both runs
validated and produced equal output, but neither is qualification evidence;
the default report also records a dirty source tree.

| Policy | Quench Score / peak RSS | Bun Score / peak RSS | Node Score / peak RSS |
| --- | ---: | ---: | ---: |
| Default | 219.12 / 43.09 MiB | 394.40 / 47.68 MiB | 385.62 / 87.89 MiB |
| Root 9 first | 211.07 / 43.09 MiB | 320.84 / 47.18 MiB | 372.46 / 88.05 MiB |

The root-first result gives no score improvement and has the same Quench RSS
as the default screen. The samples are sequential, not paired, and use three
rounds, so the apparent score differences cannot be attributed to the policy.
This is a no-go for spending more time on this admission-policy variant. The
production compiler and scoring logic were not changed by the experiment.

Raw reports:

- `quicksort-default-screen-3-round.json`
- `quicksort-root9-screen-3-round.json`
