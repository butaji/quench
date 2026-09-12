# 281 — Dead local numeric-expression supernodes

Status: complete

Compose the four-bytecode expression

`ReadLocal(source); NumberLiteral(constant); Add/Sub; WriteLocal(destination)`

as one rustc/LLVM-cooked stencil whenever all three bytecode temporaries are dead and
no control-flow edge enters the interior. The source and destination locals may be the
same recurrence cell or different cells. This is a general quoted-pattern rewrite over
every eligible numeric region; benchmark identity, source position, property spelling,
and runtime hotness never participate.

The two macro-generated templates read the guarded numeric source directly from the
local frame, consume the literal from immutable site metadata, perform the arithmetic,
canonicalize NaN exactly like the leaf operation, write only the observable destination
local, and tail-transfer with the shared connector to `site +
LOCAL_UPDATE_INSTRUCTION_COUNT`. The same template is therefore a normal morphism in
the stencil category and composes before any following arithmetic, dense access,
condition, or backedge stencil.

The selector checks consecutive PCs, exact operand flow, the canonical region number
proof, dead temporaries, and absence of interior entries. The existing five-operation
update-plus-backedge stencil retains precedence, so the broader rewrite cannot demote a
larger already-closed morphism. All instruction widths and site offsets use the named
`LOCAL_UPDATE_INSTRUCTION_COUNT`, `UPDATE_LITERAL_PC_OFFSET`,
`UPDATE_BINARY_PC_OFFSET`, and `UPDATE_STORE_PC_OFFSET` constants.

Evidence:

- all 97 release tests pass, including cooked-template execution, catalog/relocation
  coverage, same/different destination selection, malformed operand flow, interior
  entry rejection, and live-temporary rejection;
- the immutable cooked catalog grows from 122 to 124 symbols and from 11,704 to 11,808
  bytes; only a selected template is copied into a function image;
- diagnostics find four additional local-expression supernodes in Navier-Stokes and no
  new ones in Crypto for this initial local/literal family;
- the narrower same-local form measured +0.11% across Crypto/Navier-Stokes and +0.27%
  across the full suite, so it was generalized before acceptance;
- the ten-pair focused comparison in
  `reports/task281-local-expression-focused-ab-10/comparison.txt` improves the
  Crypto/Navier-Stokes aggregate from 3102.71 to 3123.88 (+0.68%); and
- the complete five-pair comparison in
  `reports/task281-local-expression-full-ab-5/comparison.txt` improves aggregate from
  1928.09 to **1941.53** (+0.70%). Every suite clears the -5% component floor.

The accepted release binary SHA-256 is
`98c720d83fcd5e9976a562f0fa174fcd257c32c11dcf88bf4c1953a90878fc35`.
This is a bounded Task 158 slice, not general register allocation: local/local,
captured/local, longer expressions, CFG value residence, spills, and edge parallel
copies remain owned by Task 158/272 rather than by an expanding handwritten pattern
table.
