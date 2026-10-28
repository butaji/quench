# Pinned ICU, canonical encoding and UTF16 error behavior

Frozen source d40beb63f passes 334/334 serial API comparisons against explicit stable Node v26.12.0 with exit zero, workspace/all-target checking, 43 framework fixtures and nine no-JIT web correctness targets. The 644-test Core result is from 820549d7a; Core source is unchanged through d40beb63f. CLI, runner and test executable hashes stayed fixed through the API run. Later shared CallSite prototypes and provider/generator integration are excluded.

The unchanged configured internal/encoding.js uses the incoming native ICU bridge. The pinned Node source builds ICU 78.3; its data SHA256 matches the official stable Node archive. The retained build provenance identifies source, compiler, platform, library and data hashes. process.versions.icu now reports the compiled version instead of the old fixed 75.1. Portable no-ICU compilation passes and omits the false ICU version.

The first focused ICU decoder comparison accidentally used an older default Node, producing a failed encoder-symbol descriptor comparison. That failed run is retained and explicitly superseded by the successful four-test run with the Node v26.12.0 binary on PATH. No production workaround was added for the oracle mismatch.

Error messages, Error.prototype.toString, default stack headers and CallSite.toString preserve UTF16 code units. Capture targets observe the original name/message getters and caching, including throwing getters. Both initial counterexamples and expanded probe input are included. The expanded input also completes the earlier native-inspection evidence bundle's probe context; results are not rewritten.

All 32 fresh matched Callgrind captures are valid with stable binary identities. Six instruction guards pass; Earley-Boyer +2.3496% and Splay +2.3675% fail the 1% guard. These are integrated findings without isolated attribution or production throughput/RSS claims. They remain open in TOD-135. This evidence does not establish total Node parity or a web benchmark win.

The atomic push of this qualified ancestor was rejected when v2-merged advanced. Neither remote ref changed; the retained push log is evidence of non-publication, not success. Subsequent integration must be qualified separately.
