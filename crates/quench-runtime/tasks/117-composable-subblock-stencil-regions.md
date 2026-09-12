# 117 — Composable sub-block stencil regions

Status: complete

Allow a semantic basic block to normalize into a sequence of compatible regions rather
than forcing one all-or-nothing template choice. The initial open direct region is the
general dead numeric local update grammar:

`LoadLocal; LoadLiteral(Number); Binary(Add|Sub); StoreLocal`

with the usual structural operand match and whole-function dead-temporary proof. Runtime
semantic-profile n-grams record about 10.16 million occurrences of this four-op form;
many are embedded inside large Crypto and Navier-Stokes blocks and therefore cannot use
the existing closed update-plus-jump stencil.

The quoted representation is a flat sequence of `Generic(start,end)` and
`Direct(start,end,template)` values. Composition remains categorical at every level:
each region is `Connector -> Connector`, region concatenation forms a block, block
concatenation forms a function, and the linked function targets a shared kernel. Generic
segments execute the one canonical semantic definition only over their declared range;
direct regions are rustc/LLVM-cooked copy-and-patch stencils. A direct guard miss enters
the existing canonical slow adapter at the failing bytecode and executes the remainder
of the original semantic block exactly once.

This is a bytecode/dataflow rewrite. It must not inspect source names, source locations,
benchmark identity, runtime heat, or literal values beyond the semantic number category.
Named constants define every instruction position and region width.

Acceptance: structural tests cover embedded Add/Sub selection, operand mismatch and live
temporary rejection; region plans are contiguous and cover each original block exactly;
all release tests and the complete V8v7 smoke pass; counters prove increased direct
coverage; an exact alternating six-run A/B against [[110-constant-condition-control-stencils]]
must improve aggregate without crossing the standing -5% per-suite floor. Reject and
revert implementation changes while retaining evidence if it fails.

## Implementation tested

- Added one immutable `QuotedRegion` sequence with `GenericBlock`, `GenericSegment`,
  and `Direct(template)` variants. Site stop points, direct counts, name snapshots, and
  final emission were all derived from this one normalized value.
- Added a canonical bounded semantic segment helper and labels at every region boundary.
- Macro-generated Add/Sub open-update stencils executed the four bytecodes directly and
  tail-called the following region; the existing slow adapter remained the only miss path.
- Added structural tests for contiguous exact block coverage, Add/Sub selection,
  mismatched slots, and live intermediates. Forty-eight release tests passed.

The extracted open Add stencil was 100 bytes / 25 AArch64 instructions. It still loaded
four site operands, checked local bounds and numeric tags, canonicalized NaN, stored the
local, advanced the site by four records, and transferred to the next region.

## Smoke-gate result

The first complete smoke (`reports/composable-subblock-smoke.jsonl`) was correct but
failed performance immediately: Richards 255, Crypto 916, and Navier-Stokes 1235,
versus recent accepted smoke values around 479, 1036, and 1239. Converting a remaining
closed suffix to direct code restored Richards to 477, but Crypto remained 910. A
general cost rule then rejected any partition producing more than one generic segment;
repeated Richards samples were still only 460, 468, and 465 versus an immediately
measured accepted baseline of 493, while Crypto remained about 915 versus 1053.

Candidate SHA-256 at rejection:
`39b10525ff640976dbbcfc460d968205adf264ddcf5a82a7d4351aaa8dab10bb`.
The smoke breach was already larger than the standing -5% suite floor, so a longer
six-run full-suite A/B was not warranted.

## Decision and learned constraint

Rejected and fully reverted. The rebuilt executable exactly matches the accepted Task
110 SHA-256:
`a239bed0433b589ed5efcd9f41029041db382e7c76c566052c196989a6887bed`.
The baseline's 46 release tests pass.

Correct sub-block categorical composition is not sufficient at four-op granularity.
The copied 100-byte island and its connector transfers cost more than the dispatch and
dead-register work they remove. A future open region must be substantially coarser and
must retain numeric values across multiple operations/effects; simply exposing more
small stencils will reduce performance.
