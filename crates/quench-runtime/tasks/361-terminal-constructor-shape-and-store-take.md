# 361 — Terminal constructor shape and ownership-taking property stores

Status: complete

Disposition: accepted and retained.

Implement the first executable slice of Task 172 for the general seven-operation terminal
form:

`LoadLocal(receiver₁) ; LoadLocal(value₁) ; SetStatic₁ ;`
`LoadLocal(receiver₂) ; LoadLocal(value₂) ; SetStatic₂ ; Return(undefined)`.

Selection is structural and bytecode-driven. It requires exact register dataflow, distinct
source locals, a terminal undefined return, and no later observation of any elided load
destination. It does not inspect source identity, benchmark identity, property spelling,
or runtime heat.

The rustc/LLVM-cooked stencil resolves both receiver shapes and both fixed slots before
performing an effect. On success it swaps each source-local word with its property-slot
word. The property receives the source's existing owner, while the displaced property
owner moves into the now-dead source local and is released by ordinary frame teardown.
This is an ownership permutation: it performs no retain, release, allocation, Rust helper
call, or per-tag ownership branch. A cache/shape/local/result miss occurs before mutation
and replays the original block through the canonical slow morphism.

The initial census showed that ownership was not the only failed obligation in Task 359.
The selected blocks initialize fields on fresh constructor receivers, so the old
own-property cache could never describe the absent slots. For an entire function that is
exactly the pure two-store terminal form and whose receivers are both `this`, derive the
two-key shape at load time. Constructor allocation installs that immutable shared shape
with two undefined fixed slots. This changes no eventual object memory use: the generic
constructor creates the same two slots, while the shape object itself is hash-consed and
shared. Preinstallation is confined to a straight-line form with no effect capable of
observing the properties before assignment.

Categorically this is one composed morphism:

`PreinstallShape ; GuardFixedSlots ; StoreTake × 2 ; ReturnUndefined`.

The shape proof and ownership proof are distinct context components, and the handler is
selected only when both are available. The quoted bytecode remains canonical; constructor
shape derivation and StoreTake tiling are derived projections.

## Evidence

- All **132** release tests pass.
- Unit tests cover structural selection, rejection of aliased source ownership, rejection
  of later register observation, constructor-shape derivation, and executable transfer of
  reference-counted string owners in both directions.
- In a 100 ms Earley-Boyer residual census, the exact target block falls from **784,948**
  generic entries before constructor-shape installation to **2** afterward (the initial IC
  fills). The candidate smoke score in that run is **2939**.
- AOT disassembly contains guards, loads/stores, two raw swaps, the site advance, and only
  patched tail branches; it contains no call instruction.
- Candidate binary SHA-256:
  `a451decd3baf454dc9b892d2ea125db38aca6d318d09e209b5466e24b5841dde`.

Three alternating complete-suite 200 ms repetitions against the accepted Task 358 binary
measure:

| Suite | Baseline | Candidate | Change |
|---|---:|---:|---:|
| Richards | 939 | 916 | -2.45% |
| DeltaBlue | 950 | 933 | -1.79% |
| Crypto | 1815 | 1824 | +0.50% |
| RayTrace | 2021 | 1951 | -3.46% |
| Earley-Boyer | 2814 | 3291 | +16.95% |
| RegExp | 3763 | 3702 | -1.62% |
| Splay | 4077 | 4060 | -0.42% |
| Navier-Stokes | 7013 | 6976 | -0.53% |
| **Geometric aggregate** | **2368.60** | **2385.72** | **+0.72%** |

The result clears the standing -5% component and -3% aggregate floors and is retained.
Artifacts are in `reports/task361-ownership-swap-shape-ab-200ms-3/`; the residual census
is `/tmp/task361-earley-shape.out` for this workspace run.
