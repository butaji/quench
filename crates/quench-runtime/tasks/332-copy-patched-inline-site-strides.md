# 332 — Copy-patched `InlineSite` successor strides

Status: complete

Make metadata-cursor advancement an explicit copy-patch fact instead of baking
`site.add(1)` into every ordinary numeric-region stencil. The build-time rustc/LLVM cooker
recognizes a named AArch64 `ADD (immediate)` placeholder and records its offset in the
immutable `StencilTemplate`. Instantiation patches the byte distance for the concrete
`StencilInstance`; the template and shared kernels remain unchanged.

The quoted region tiler derives the next executable semantic PC. A non-elided stencil can
therefore absorb a consecutive run of `RegionOp::Elided` identities into its successor
stride, so composition removes the machine-code nop seams as well as their semantics. If
an unshifted AArch64 immediate cannot encode the distance, the general one-site composition
is retained. Jumps keep their existing symbolic target rules, and a run following a coarse
supernode retains one patchable nop rather than corrupting metadata state.

Implementation:

- `stencil-aot/site_holes.rs` is the single definition of the immediate width, maximum,
  and placeholder; there are no numeric literals hidden in matching code;
- `build.rs` extracts 47 site-advance relocations and emits them in `RustcStencil` records;
- `src/dynjit.rs` patches them when instantiating and folds consecutive identities;
- focused tests prove extraction, immediate rewriting, and quote-level folding.

All 123 debug, release, and stress-GC tests pass. An initial report was rejected before use
because both inputs had the same SHA-256; it is preserved as
`reports/task332-invalid-identical-binaries`. The valid distinct-binary three-pair, 300 ms
complete-suite A/B at `reports/task332-site-stride-v2-full-ab-3/comparison.txt` measures
2123.27 to 2133.61 (+0.49%), with every component above -1.04%. Baseline SHA-256 is
`701f9a3ca983f4c0be651d974a2225e3c8d8c7619b07637120099c69ae3df65e`; candidate SHA-256 is
`7877b5fc01f4ceeb206e1c0e03658b9caff957beb40b7287b67408d82258d9ce`.

The emitted-code evidence matches all 16 selected sites: Crypto drops 24 bytes for three
forwarded loads, Earley-Boyer drops eight bytes for one, and Navier-Stokes drops 88 bytes for
twelve; the five unaffected suites have identical code-byte counts. This infrastructure is
accepted and 2133.61 is the best measured absolute checkpoint, but broad-suite movement is
mostly measurement noise because five suites execute no changed stencil. The target remains
10000.

This is the Lisp staging boundary made concrete: elimination stays quoted data; only final
template instantiation performs the effectful byte patch. Categorically the patched stride
is an instance obligation on the same `Connector -> Connector` morphism, not a new execution
kind.
