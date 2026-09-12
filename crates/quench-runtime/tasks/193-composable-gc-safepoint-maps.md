# 193 — Composable precise GC safepoint maps

Status: planned

Make GC obligations first-class stencil metadata beside holes, labels, and entries:
`GcPoint { code_offset, live_registers, live_frame_slots, effect: NoGc | MayGc }`.
Composition shifts and concatenates points; `freeze` preserves them; final linking emits
the root table. Arithmetic and guarded property-hit regions remain `NoGc`; allocation,
unknown calls, and explicit effect kernels are `MayGc`.

The first sound implementation canonicalizes all GC-live values into known frame slots
before `MayGc`; register roots may be added only with verified rustc/AArch64 locations.
Task 172 derives liveness/ownership and Task 164 supplies reconstructible side-exit state.

These maps are also the proof that permits Task 09/162 values in registers and local
slots to be non-owning raw handles. Every `MayGc` edge must enumerate all such handles;
between safepoints, arbitrary stencil-local moves require no retain/release operation.
Tests must therefore include collection immediately after a value's last heap edge is
removed while its only remaining references are mapped frame/register roots.

Acceptance: category-law tests cover identity/association of metadata composition;
collection at every safepoint preserves all and only live roots; frozen composites retain
maps; object interior pointers cannot cross `MayGc`; scan work falls on allocation-heavy
suites without correctness or A/B regression.

Primary sources: LLVM statepoints <https://llvm.org/docs/Statepoints.html> and stack maps
<https://llvm.org/docs/StackMaps.html>.

Round-twelve refinement: Task 171's materialization bit is the one source of truth for
whether a live value's canonical frame slot is current. Ordinary stencil boundaries do
not force stores. Only `MayGc`/`MayObserve` edges request a materializer; a proved
non-reference needs no root entry, while Task 261's known tag bits may prove a root
without another dynamic tag test. This mirrors the on-demand tagging/materialization
strategy that measured within 0.9–4.9% of an ideal no-tag baseline in Wizard-SPC:
<https://arxiv.org/pdf/2305.13241>.
