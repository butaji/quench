# v2-cloud-c* integration check

User-directed integration on v2-cloud, source merge a200755e6. The c head
8c69d6aeb is merged; c2 head f71a0ee24 is already an ancestor and has no unique
commits to merge. The current record-aware generational collector and direct
Value families remain authoritative. The old collector worker files are
superseded by the integration collector. Call-plan, local promotion and numeric
changes from the speed lane are retained.

Two debug-only integration defects were repaired: full-GC reference verification
now clears/restores both record mark maps, and shared frame return recognizes
Wasm function zero as a defined function. No benchmark-specific behavior.

Checks: workspace/all-targets and aggregate/memory/trace feature checks pass;
runtime lib 582/582 with debug assertions and opt-level=0 (test-only override);
interpreted Wasm 67,124/67,124. Affected Test262: stage 10 11,087/11,102;
stage 11 217/217; Array 3,081/3,081; Function 508/509; Number 340/340;
FinalizationRegistry 47/47; WeakMap/WeakRef/WeakSet 255/255. The 15 stage-10
assertions also fail with unchanged declared harness input on the clean pinned
baseline and Node 26.10.0. The native stage-48 runner has identical 509-case
outcomes on clean baseline 81b571194 and the merge (one cross-realm failure).
These failures remain failures; no harness or host shims were used.

JSON provenance reflects when checks ran, including the precommit dirty merge.
Later production rebuild differs only in debug-only fixes. Its binary hashes
are in provenance.json. This is an integration result, not a performance win.

Final production all-eight stock smoke: 8/8 valid and output-equal, one round
per fixture, pinned QuickJS 2026-06-04, Bun 1.4.2 (useJIT=false), Node 26.10.0
(--jitless). This short run is a semantic smoke, never qualification or a
performance retention claim. Report includes exact engine/runner identities.
