# Rejected EarleyBoyer built-in `@@hasInstance` lookup fast path

This candidate bypassed symbol lookup and the native call only when the
constructor was a non-proxy function whose internal prototype was the VM's
`Function.prototype`, it had no own `@@hasInstance`, and the inherited
property was a data property containing Quench's exact native
`Function.prototype[@@hasInstance]`. Other prototype chains, own overrides,
accessors, and proxies kept the generic property/call path. The direct path
reused `ordinary_has_instance`.

The Node v24.19.0 oracle covered ordinary and bound functions, own custom and
non-callable overrides, a custom function prototype chain, proxy traps, and
ordinary positive/negative checks. Baseline and candidate had the same
outcomes and observable traces as Node and matched each other. Three error
message strings differ from Node in both unchanged baseline and candidate.

Three alternating production pairs on the pinned EarleyBoyer input matched
benchmark output but rejected the candidate:

- Median Score: 421 baseline, 404 candidate; paired median delta −16.
- Median maximum RSS: 41,639,936 bytes baseline, 47,742,976 candidate; paired median delta +6,111,232 bytes.
- Candidate Score was lower and RSS higher in every pair.

The candidate binary grew by 96,992 bytes. This was a three-pair rejection
screen, not qualification evidence. I removed the fast path. Pair rows, hashes,
raw output, oracle and source patch are preserved here.
