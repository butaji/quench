# 264 — Use-directed Word32 truncation islands

Status: planned

Add one backward representation-demand analysis over Task 171's RegionPlan. Each SSA use
requests a finite observation mode such as `Unused`, `Boolean`, `Word32`, `Float64`, or
`Tagged`. Joins take the least representation that satisfies every live use. A bitwise,
shift, `Math.imul`, or other ToInt32 consumer requests only the low 32 bits; compatible
producers propagate that demand to their inputs and select wrapping I32 derivatives
from Task 149.

The important case is an integer island, not one specialized opcode:

`guard/unbox -> Word32 graph -> box/materialize only at an observing boundary`

Within the island, additions/subtractions/multiplications whose consumers observe only
Word32 do not need a Float64 round-trip or an overflow exit. A use that can observe the
full Number, NaN, infinity, fraction, or signed zero stops propagation and requests the
correct conversion/materialization. Task 214 remains the forward range/edge-case fact;
Task 261 remains bit knowledge; this task is the dual, consumer-driven demand. One
canonical `AbstractValue` stores their product.

Categorically, this is a contravariant analysis of uses: demands flow from codomain to
domain while ordinary facts flow from domain to codomain. The two meet before stencil
tiling, and the chosen `Stencil<GammaIn, GammaOut>` makes the representation transition
explicit. There is no Word32 interpreter and no source-pattern selector.

Acceptance: property tests cover JS ToInt32/ToUint32, shifts, wraparound, negative zero,
NaN, infinity, mixed full-Number and Word32 consumers, joins, and loops; diagnostics
count eliminated conversions and overflow checks; disassembly of representative
bitwise chains contains one entry conversion and one required exit materialization;
Crypto and complete-suite alternating A/B pass.

Primary implementation source: V8's `SimplifiedLowering`, where a
`TruncatingWord32` use changes compatible arithmetic to pure `Int32` operations:
<https://chromium.googlesource.com/v8/v8/+/f45c842fe1b011f7fde237112067dcc999b71dd3/src/compiler/simplified-lowering.cc>.

