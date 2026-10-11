# Splay weak-handle generation removal

Host: Apple M4 arm64 macOS, 10 logical CPUs, 16 GiB. This is fixed-work
diagnostic evidence, not stock-harness qualification.

The baseline is the pinned production binary built from runtime source
`ba3eee054` (SHA-256
`b1222e3d7f636d9d9863f90eeb1d5b63b7ce0f1fb8833657cd0818ecc9a5cd62`). The
candidate is production code from dirty source atop evidence-only HEAD
`61371b19c` (SHA-256
`29ad4b2fb0683a1ecee79847b47dde07fada568993eaa79f0a861372df29db01`). The
measurement runner is pinned at
`f8e02485d1724f130fb318fdf26089896dcdc540c03131173464d92301765bfa`.

`splay-fixed-work-11.json` records 11/11 valid, output-equal, contention-clean
pairs at 300 runs per benchmark. Candidate marginal cycles were 5.224M versus
5.270M baseline (median paired change −0.62%; the ratio of the two marginal
medians is −0.87%). Marginal instructions changed −0.35%. Median work max RSS
fell from 122,748,928 B to 116,604,928 B (−6,144,000 B, −5.0%); setup-only
median RSS fell from 68,763,648 B to 65,585,152 B (−3,178,496 B).

The all-eight fixed-work report has 11/11 valid and clean pairs for every
fixture. No fixture has a detected cycle regression; marginal instructions
are flat or lower across the set. The guard's uniform +0.5% RSS budget flags
RegExp at +0.992%, while the other fixtures are within budget. Splay improves
by 4.985% RSS.

I measured RegExp same-binary RSS noise before deciding whether that flag is
real. Candidate-vs-candidate A/A has 11/11 clean pairs and a median paired RSS
change of +0.043%, with a 95% paired bootstrap interval of −1.051% to +1.121%.
These intervals use 20,000 paired percentile-bootstrap replicates of the
median relative RSS change and the fixed seed from the guard. The baseline A/A
ran during a Deno burst (3/11 cycle-clean; excluded as a cycle control); its
valid-sample RSS interval was −2.748% to +1.023%. RegExp A/B's +0.992% median
is inside the candidate A/A interval; its own 95% interval is +0.258% to
+1.168%.
Applying the established per-fixture RSS rule `max(0.5%, A/A band)` gives a
1.121% RegExp budget, so this result is within measured RSS noise rather than
a regression beyond noise. The raw fixed-budget guard decision remains
preserved at `all-eight-guard.decision.json` and says `rejected` solely because
it uses a uniform 0.5% threshold.

The RSS reduction is consistent with removing the per-slot `u32` generation
vector. The Splay-only fixed-work result is a diagnostic, not a stock-harness
win. The prior M4 stock distances remain 2.32× speed / 2.00× RSS until a clean
stock-harness Splay retake.

Correctness checks completed before this gate: runtime lib 544/544, Test262
FinalizationRegistry stage 47 47/47, WeakRef stage 80 29/29, canonical Wasm
67,124/67,124, and the Node oracle probe for a dead unregister token returning
`false` on a replacement object.
