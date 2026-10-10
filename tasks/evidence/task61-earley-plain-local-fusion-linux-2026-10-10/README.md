# Earley plain-local store/load fusion rejected

The candidate fused `StoreLocalPlain → LoadLocalPlain` only when both
instructions named the same local, the store had no assignment-result target,
and the load had no numeric-update side effect. This pair occurred 54.9M
times (7.3% of physical dispatches) in the instrumented Earley profile, which
motivated an attempt to remove one interpreter dispatch per pair.

A three-pair alternating production screen produced equal semantic output, but
Score fell in all three pairs: 353→352, 364→358, and 384→377. RSS fell in one
pair and rose in two. Since the Score direction was uniformly negative, the
candidate was removed without expanding to eleven pairs.

Node v24.19.0 matched Quench for local assignment results, repeated reads,
negative zero, numeric loops, and closure-visible mutations. See the paired
screen, candidate patch and oracle in this directory. The screen is diagnostic
and does not qualify EarleyBoyer.
