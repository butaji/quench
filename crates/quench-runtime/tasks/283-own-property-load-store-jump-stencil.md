# 283 — Own-property load/store/jump stencil

Status: complete

Replace the normalized four-bytecode block

`LoadLocal(receiver); GetStatic(key); StoreLocal(destination); Jump(target)`

with one rustc/LLVM-cooked stencil. The direct arm borrows the receiver from its local,
checks the site's monomorphic own-property shape/slot IC, loads the fixed slot, writes an
immediate result directly to the destination local, and tail-transfers to the symbolic
CFG target. Heap-owned results, inherited properties, empty/mismatched ICs, invalid
locals, and heap-owned overwritten destinations enter the canonical slow adapter at the
first bytecode so full semantics and ownership remain unchanged.

Selection is a pure rewrite over normalized bytecode structure, exact operand flow,
global temporary liveness, and CFG block boundaries. It never inspects property spelling,
source location, benchmark identity, or runtime execution count. The closed stencil is
an ordinary `Connector -> Connector` morphism compatible with every other stencil and
shared kernel.

Use named instruction indices/counts in both the selector and AOT semantic source; no
layout or bytecode magic numbers.

Acceptance: selector near-miss/liveness tests, cooked fast-path execution, 98+ release
tests, complete-suite correctness, direct selection evidence, and alternating A/B
improvement against Task 281's accepted binary. Reject and remove if the benchmark gate
does not improve.

Result: accepted. The selector adds 18 direct blocks / 72 direct bytecodes in
Earley-Boyer and one block / four bytecodes in Splay; all other suite selection counts
are unchanged. The catalog adds one shared 140-byte template (124 to 125 symbols,
11,808 to 11,948 bytes). All 99 release tests pass, including positive selection,
operand-flow/liveness rejection, and execution against a real shaped object/IC slot.

The selected-suite ten-pair comparison at
`reports/task283-property-load-selected-ab-10/comparison.txt` improves Earley-Boyer
0.47%, Splay 0.74%, and their geometric aggregate 0.60%. The complete five-pair
comparison at `reports/task283-property-load-full-ab-5/comparison.txt` improves the
exact aggregate from 1896.78 to 1900.16 (+0.18%); every suite remains above the -5%
component floor. The accepted binary SHA-256 is
`dca5e65592d3fcdb44f46b143039a31ac72559413b63076ddf4ecd5e9efb28b5`.
