# EarleyBoyer StoreLocalPlain-to-LoadConst preflight

I added a candidate superinstruction that preserves the store's optional
assignment-result write before loading the following constant. The
aggregate-profile binary built successfully and ran the pinned fixture, but
its census counted **zero** `SuperStoreLocalPlainLoadConst` executions among
737,899,774 physical dispatches. This means the earlier high dynamic
transition count does not create enough fusible straight-line work here. I
stopped before production scoring and removed the candidate.

`candidate.patch` preserves the implementation, `census.stderr` preserves the
aggregate census, `profile.stdout` records the instrumented fixture result,
and `summary.json` records the fixture and binary hashes. The profile run's
Score/RSS are instrumented and excluded from performance claims; no production
pairs were run.
