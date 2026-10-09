# Function-name cache rejection on Linux

The candidate cached the initial immutable `Function.name` string value per
compiled function ID in the program store. Function objects still received
independent properties, and later inferred or assigned names kept the existing
`SetFunctionName`/property path. Node v24.19.0 and Quench matched fresh closure
identity, both names, source text, and calls.

Eleven alternating production pairs on EarleyBoyer found no joint Score/RSS
win:

- Median Score was 204 for both builds; paired 95% interval for the median
  delta was -3 to +6 points. The candidate had lower Score in 7/11 pairs.
- Median maximum RSS was 50,700,288 to 50,712,576 bytes (+12,288); paired
  95% interval was -540,672 to +868,352 bytes. Candidate RSS was lower in
  4/11 pairs.
- Output matched in all eleven pairs.

Reject this cache. It does not establish a Score gain or an RSS reduction.
The ordered samples, binary hashes and runner are in `earley-pairs.json`,
`earley-pairs.jsonl`, and `runner.py`.

The Node oracle matched exactly. Test262 Function/GeneratorFunction stages
48–49 passed 532/532, AsyncFunction stages 36–37 passed 41/41, and the
function-code, export, import, and module-code selections passed 947/947. The
Stage 10 sweep passed 11,087/11,102; its 15 failures are the known baseline
compound-assignment and prefix/postfix cases.

The source change was removed after this result. Its compressed patch is retained as `source.patch.gz` for experiment
provenance.
