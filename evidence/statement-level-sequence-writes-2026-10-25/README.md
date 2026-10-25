# Statement-level sequence, conditional and logical writes keep promoted locals uncopied

Host: Linux x86_64 cloud container, quench-node no-JIT, callgrind instruction counts per
iteration ((N=3) - (N=1)) / 2, `seam.js` driver over the SeaMonster sjcl benches.

| bench | before (5db3b07a2) | after |
|---|---|---|
| stanford-crypto-sha256 | 911.7M | 816.3M (-10.5%) |
| stanford-crypto-pbkdf2 | 876M | 792M (-9.6%) |
| stanford-crypto-aes | 1294M | 1294M |

sjcl's SHA-256 round function: 221 -> 169 instructions, copies of promoted locals 42 -> 6.

Oracle: `genseq.py SEED` writes random functions mixing reads with nested assignments,
updates, compound assignments, comma sequences, conditionals, logical operators and calls,
in statement and operand positions; seeds 1-60 (no-JIT) and 61-90 (no-JIT and `--jit`)
print the same as Node v22 (`node gs.js` vs `quench-node gs.js`).
