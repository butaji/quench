# Doxbee Promise native-entry census

One-round JetStream 3 diagnostic with `--jit --jit-diagnostics` completed with
valid output. The run used source revision `97d7af5e171eead22752d49afdb3171f4426171e`
and the pinned JetStream checkout recorded in the raw report.

The diagnostic emitted 78 JavaScript function rows. Across the reported
functions it counted 10.8 million entries and zero native entries. The main
fixture functions account for nearly all measured interpreter time:

| Function | Entries | Inclusive interpreter time | Outcome |
| --- | ---: | ---: | --- |
| `doxbee` | 1,200,000 | 21.38 s | rejected: unsupported activation/function kind |
| anonymous closure | 1,200,000 | 10.89 s | rejected: unsupported activation/function kind |
| `runIteration` | 1 | 27.36 s | compile attempt, rejected by lowering/codegen |
| `createQuery` | 1,200,000 | 1.77 s | rejected: unsupported activation/function kind |

The branch/type-check census in this workload is not evidence of redundant
guards. ISOP would not address the observed gap. Native-entry coverage and
closure/call support are the dominant JIT issues here. `runIteration` includes
array allocation, repeated calls, and a Promise aggregate, so extending the
current numeric-only artifact directly would require a much broader effectful
ABI and fallback design.

Raw report: `promise-1round.json`.
