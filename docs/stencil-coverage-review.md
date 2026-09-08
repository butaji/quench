# Stencil coverage and composition review

Reviewed the working tree on 2026-09-08, including existing uncommitted stencil
changes. This is a catalog and architecture review, not a proof of every native
instruction or a measured performance study.

The main opportunity is to make existing operations compose across more valid
dataflow shapes. Adding more whole-function recipes alone grows the catalog
without providing comparable coverage. Keep canonical IR semantics and CFG facts
as the authority; derive bounded physical plans from them.

## Coherent stencil model

The catalog is a finite machine-code grammar, not a list of JavaScript programs.
An execution-profile fixture may expose a missing cover, but its source, names,
constants and iteration count never become selection facts. Four data concepts
are sufficient:

| Concept | Sole authority | Lifetime |
| --- | --- | --- |
| Semantic operation | Existing residual opcode declaration, operands, effects and control | Immutable code |
| Fact | `Proven`, `Guarded` or `Unknown` representation/layout/range/dependency fact | Existing quickening or disposable analysis |
| Physical recipe | Target, typed ABI roles, constraints, clobbers, holes, links, bytes and cost | Build-time catalog |
| Cover | Residual PCs, facts, recipe IDs, bindings, transfers and exact exits | Bounded disposable plan |

Do not add a fifth program representation. The disposable value/use graph is a
view of residual PCs and facts; it cannot invent JavaScript semantics. Covering
turns increasingly precise data into a recipe. Publication remains a separate
effect after the complete cover, links and ABI have been verified.

### Family grammar

Every retained or new recipe belongs to one of these parameterized families.
The dimensions are data, not branches embedded in selectors.

| Family | Parameters that define valid variations | Required broken-fact boundary |
| --- | --- | --- |
| Value transfer | tagged/i32/u32/f64/pointer representation; local, constant, register or spill location; initialized/TDZ; owned/borrowed live-out | deleted/immutable binding, unsupported representation, live owner not materializable |
| Scalar expression | operator; ordered operands; register/immediate/memory form; range and conversion proof; result representation | coercion, overflow or signed-zero/NaN case outside the declared form |
| Predicate/control | truthiness/nullish/comparison condition; fallthrough or branch continuation; join live-outs | observable conversion, illegal entry, unsupported exception/suspension edge |
| Memory access | object/array/typed-memory base; layout/kind; slot/index/address form; read/write; receiver and dependency set | descriptor/prototype mutation, hole, bounds, resize/detach, aliasing or reentry invalidation |
| Call boundary | target certainty; receiver; argument window; result continuation; helper effects | callee/realm/receiver change, construct/async/generator/class or unsupported reentrant interior |
| Counted region | induction start/step/bound; carried values; body covers; stores; exits; interruptible backedge | incompatible loop representation, invalidated guard, unsupported effect or precise committed exit |
| Allocation boundary | allocation-site/layout fact; ordered initializers; escape and materialization requirements | setter/prototype/identity/escape/throw condition not proven safe |

Strings, RegExp, BigInt and exotic objects normally terminate at a complete
boundary recipe. They join this grammar only when their existing semantics expose
a proven reusable operation family; they do not create a parallel stencil VM.

Existing names such as `typed_lane`, `two_state_i32`, `matrix_reduction`,
`dense_copy` and `branch_recurrence` describe current large terminal covers.
They are not new semantic families. Migrate their selection to the shared
counted-region graph and keep a large cover only when its measured saved work
beats general composition. A “kernel” is therefore only a large stencil cover;
it owns no alternate admission, semantics, cache or lifetime system.

### Covering and composition

The single bounded selection path is:

```text
canonical CFG + declared operand/effect facts
  -> disposable use/def graph with effect scopes
  -> monotone fact strengthening and safe folding/CSE/DCE
  -> bottom-up candidate covers from the finite recipe catalog
  -> last-use placement in a small fixed register-role vocabulary
  -> costed re-cover with actual roles and continuations
  -> symbolic layout, typed relocation, verification and publication
```

Candidate cost counts removed dispatch, guards, decoding, boxing and ownership
traffic, then subtracts transfers, spills, exits, code bytes and render work.
The largest cover is not automatically the winner. LLVM can optimize inside one
offline Rust recipe, never across copied fragments; cross-fragment residency is
established only by explicit input/output/clobber/continuation contracts.

The value graph must accept alpha-renamed registers and locals, constants on
every semantically valid side, aliases, harmless pure prologues, nonzero loop
starts, zero/one/many iterations and all bounded literal values admitted by a
recipe. It must distinguish evaluation order, live-out aliases, conversions,
descriptor/prototype dependencies, backing identity, exception points and
effects. Those either select another family member or reject before entry; they
never cause a fixture-specific matcher.

### Execution-profile acceptance

Each native family needs a small variation basis in
`crates/quench-runtime/testdata/execution_profiles`: a canonical case, renamed
bindings, operand/immediate variants, boundary sizes and a fact-breaking ordinary
fallback. JSON describes the ideal family/cover and eliminated work. It must not
encode the current miss merely to turn the suite green. A case passes only when
its self-checking JavaScript result, selected physical route and invocation-local
profile all agree.

Before adding a recipe, name its family dimensions and show which existing
recipe cannot represent them. Before retaining a large terminal cover, compare
it against general composition on at least one held-out source shape. This keeps
performance cases as architectural fitness tests rather than production input.

## Complete catalog inventory

There are **84 declarations**: 27 Rust leaves, 40 assembly recipes, and 17
composed/bridge rows. These are distinct from the 44 runtime admission variants.
An admission variant can use ordinary Rust, a generated whole function, or a
composed image; its name does not prove native execution.

| Catalog group | Rows | Declaration names |
| --- | ---: | --- |
| Rust numeric arithmetic | 7 | `loop`, `subtract`, `multiply`, `divide`, `add_const`, `negate`, `increment` |
| Rust comparisons | 8 | `compare_equal`, `compare_not_equal`, `compare_less`, `compare_less_equal`, `compare_greater`, `compare_greater_equal`, `compare_equal_word`, `compare_not_equal_word` |
| Rust bitwise | 7 | `bitwise_and`, `bitwise_or`, `bitwise_xor`, `shift_left`, `shift_right`, `shift_right_zero`, `bitwise_not` |
| Rust predicates and whole recipes | 5 | `truthy_number`, `bitwise_shift_mask_return`, `number_classify_branch_return`, `nullish_truthy_branch_return`, `guarded_missing_property_return` |
| Linked numeric assembly | 5 | `fallthrough`, `sub_fallthrough`, `mul_fallthrough`, `div_fallthrough`, `add_chain` |
| Word control assembly | 4 | `bool_branch`, `truthy_bool_branch`, `return_word`, `word_const_fragment` |
| Compare/branch assembly | 6 | `compare_equal_branch`, `compare_not_equal_branch`, `compare_less_branch`, `compare_less_equal_branch`, `compare_greater_branch`, `compare_greater_equal_branch` |
| Numeric loop assembly | 9 | `array_numeric_loop`, `array_numeric_fill_loop`, `affine_i32_loop`, `i32_counter_loop`, `numeric_integer_loop`, `numeric_floating_loop`, `numeric_bitwise_loop`, `numeric_independent_loop`, `numeric_mixed_loop` |
| Property assembly | 3 | `property`, `prototype_property`, `store_property` |
| Array kernel assembly | 6 | `array_get_number`, `array_set_number`, `array_get_inc_number`, `array_numeric_update`, `array_numeric_update_const`, `array_loop_body` |
| Tagged movement assembly | 3 | `move`, `load_local`, `store_local` |
| Word leaves in assembly | 4 | `truthy_pointer_word`, `load_const`, `nullish_word`, `truthy_word` |
| Composed array loops | 3 | `dense_numeric_copy_loop`, `ordered_f64_reduction_loop`, `conditional_f64_reduction_loop` |
| Canonical handler bridges | 14 | `dispatch`, `loop_glue`, `loop_body`, `binary_glue`, `update_return`, `call`, `call_n`, `arithmetic_glue`, `get_property`, `set_named`, `get_index`, `set_index`, `get_index_inc`, `for_i` |

Sources: [leaf declarations](../crates/quench-runtime/build_stencil_catalog/declarations_rust_leaf.rs),
[assembly declarations](../crates/quench-runtime/build_stencil_catalog/declarations_rust_assembly.rs),
[composed declarations](../crates/quench-runtime/build_stencil_catalog/declarations_composed.rs).

### Runtime admission migration matrix

The 44 `NativeAdmission` variants reduce to the grammar above. “Merge” means
reuse the body or boundary but replace its independent selector with the shared
coverer. It does not mean deleting tested code before its replacement works.

| Family | Current admission variants | Disposition |
| --- | --- | --- |
| Value transfer | `LoadConst`, `Move`, `LoadLocal`, `StoreLocal` | Keep typed leaves; derive location/ownership variants from one transfer recipe declaration |
| Scalar expression | `Binary`, `Unary`, `AddChain`, `LocalBinary`, `NumericDag`, `I32Pattern` | Merge selection into one bounded expression graph; retain profitable leaf/fused terminal covers |
| Predicate/control | `Truthiness`, `Nullish`, `LocalPredicate`, `NumberClassify`, `NullishTruthy`, `MissingProperty` | Express predicate plus continuation as data; compose patched constant/result arms rather than fixed literals |
| Guarded memory | `Property`, `StoreProperty`, `LocalProperty`, `PropertyNumeric`, `DenseFill`, `DenseUpdate`, `DenseCopy`, `Reduction` | Share receiver/index identity and dependency facts; keep object/array commits and ownership boundaries explicit |
| Counted region | `IntegerLoop`, `FloatingLoop`, `BitwiseLoop`, `IndependentLoop`, `MixedLoop`, `LocalAffineSum`, `LocalRecursiveSum` | Replace fixed windows and `start == 0` with one counted-region analysis; bodies become expression/memory covers |
| Call boundary | `CallReturn`, `ForwardCall`, `ForwardPair`, `FreshObjectCall`, `MethodCall`, `PropertyPair`, `PropertyReturnCall`, `PropertyStoreCall`, `PrototypeCall` | Share callee/receiver/argument facts and exact continuation; retain explicit effect and ownership commits |
| Semantic boundary | `StringConcat`, `StringBuiltin` | Keep portable complete helpers; compose only proven conversions and result continuations |
| Physical infrastructure | `Region`, `Dispatch` | `Region` publishes verified covers; `Dispatch` remains canonical bridge fallback and is never counted as native semantic coverage |

The separate function-entry recognizers for typed-lane, two-state, matrix,
switch, nested-XOR, branch and boolean reductions are additional current covers,
not additional families. Their hand-ordered `try_execute_*` chain is a migration
hotspot: select candidates from one counted-region graph, rank them once, and
record the chosen family/cover from that plan. Do not append another recognizer
when an execution-profile case is red.

## What coverage means

Track these separately: declared operation coverage; valid physical artifacts per
target; static selection; successful runtime guards; actual native entries;
operations completed per entry; and correct fallback after a guard breaks.
The `dispatch` row lists compact opcodes but executes canonical handlers.
Counting it as native implementation coverage would obscure the remaining work.

The [extraction pipeline](../crates/quench-runtime/build_stencil_artifacts/pipeline.rs)
extracts assembly recipes only for AArch64; Rust leaves use a separate extraction
path. Several rows intentionally have empty legacy bytes. The
[execution policy](../crates/quench-runtime/src/stencil_policy.rs) disables ARM
stencils by default and keeps its optimizing driver disabled even with ARM
stencils enabled. Target policy, executable metadata, selected artifacts, and
typed admission must all agree before a coverage claim is meaningful.

## Composition improvements, in priority order

1. **Consolidate bounded value tracking.**
   [BlockValueGraph](../crates/quench-runtime/src/stencil_value_graph.rs), the
   [numeric DAG](../crates/quench-runtime/src/stencil_numeric_dag.rs), and the
   [property/numeric cover](../crates/quench-runtime/src/stencil_property_numeric.rs)
   each track aliases, sources, and numeric producers. Share value identity and
   def-use traversal over canonical instructions; keep physical recipes and
   property guards separate. This should let existing arithmetic work through
   moves, repeated operands, constants on either side, and guarded property
   inputs without another selector for every expression shape.

2. **Recognize loop structure through CFG and dataflow.**
   Floating, bitwise, independent, and mixed loop selectors require `start == 0`
   and fixed instruction windows. For example,
   [select_floating_loop](../crates/quench-runtime/src/stencil_numeric_floating_loop.rs)
   matches a 35-instruction prefix. Extract induction, bound, carried values,
   recurrence, and exits once from existing CFG facts. First admit the same
   recurrence after harmless prologue code or through aliases; then generalize
   body operations. Preserve number operation order, integer-conversion proofs,
   interrupt checkpoints, and exact state at each exit. Do not merely relax the
   opcode-window checks without replacing their proofs.

3. **Extend continuation contracts incrementally.**
   [Linear composition](../crates/quench-runtime/src/stencil_region_builder.rs)
   currently requires two-operation rows, matching external/internal ABIs, no
   helpers, and a valid fallthrough tail. The internal ABI vocabulary is only
   `None`, two F64 accumulator forms, and `WordX0`.
   [Word composition](../crates/quench-runtime/src/stencil_word_composition.rs)
   handles a branch with returns or constant arms. Add typed input/output
   register roles and explicit edge transfers for arithmetic-to-comparison and
   comparison-to-branch compositions before attempting arbitrary control flow.
   Reuse `RegionControlPlan`, `selected_transfers_by_role`, and the verified layout
   path. External ABI equality alone cannot authorize concatenation.

4. **Turn fixed decisions into parameterized compositions.**
   The whole nullish/truthy recipe embeds results 41 and 7; number classification
   embeds 1 through 4. Their selectors must prove those literals. Build the same
   decisions from guarded predicates and patched constant arms so other result
   values can reuse the implementation. Keep NaN, negative zero, nullishness,
   and truthiness semantics explicit. Use the existing word-constant fragment
   rather than cloning the whole recipe for another pair of constants.

5. **Share guards across call/property plans, with explicit effects.**
   Call-return, forwarding/pair calls, receiver/argument selection, fresh-object,
   method, property-pair, property-return/store-call, and prototype-call plans
   repeat selection and guarded execution around bounded function facts.
   Share proven callee identity, operand binding, and side-effect-free property
   reads. Retain explicit ownership and commit code: numeric and property commits
   differ because a property result may need retaining before its receiver loses
   its last owner. An effectful call/store must not return a guard miss that
   causes already-completed effects to execute again.

6. **Generalize portable covers before adding native bodies.**
   String concat/builtins, property/numeric evaluation, ordered neighbor
   recurrence, local affine/recursive sums, and several call plans use guarded
   Rust execution. The string-builtin cover, for example, recognizes a fixed
   three-search sequence. Generalize safe operand/dataflow composition while
   reusing canonical string helpers. Only add a native implementation when
   production measurements justify its code size and boundary cost.

7. **Improve bounded DAG register use before raising limits.**
   The numeric DAG caps selection at 32 instructions, 24 values, and 3 inputs.
   Its [AArch64 emitter](../crates/quench-runtime/src/stencil_numeric_dag_aarch64.rs)
   consumes fresh scratch registers without recycling last uses and emits a
   literal for each literal use. Derive last uses and reuse registers before
   enlarging arrays or adding spills. Deduplicate literals by exact bits so
   signed zero remains distinct. Measure selection time, emitted bytes, and
   successful admission to justify either change.

## Macro changes made in this review

| Macro | Single declaration supplies | Net Rust line reduction |
| --- | --- | ---: |
| `bridge_region!` | Name and semantic window; derives the identical trampoline bytes, pointer holes, ABI, and single entry for 14 rows | 116 |
| `native_admission_catalog!` | Variant, plan type, and debug label; derives the 44-variant enum, shared ownership, retained metadata accounting, and `Debug` | 142 |

Total: **258 fewer Rust lines**, including the new 75-line
[admission catalog](../crates/quench-runtime/src/machine_native_admission.rs).
Admission order, runtime guards, JavaScript semantics, and native bytes are
unchanged. A standalone Rust comparison verified all 17 composed declarations
are field-for-field identical before and after expansion.

The existing `rust_leaf_catalog!`, `rust_assembly_catalog!`, `region_abi_catalog!`,
`invoke_shared_entry!`, and typed admission accessor macros already address useful
repetition. Extend those before introducing a general stencil DSL.

Further worthwhile candidates:

- A shared Rust context declaration that derives runtime `repr(C)` records and
  the source/offset declarations used by assembly extraction. Context structs
  currently recur inside template strings, and some templates still hard-code
  offsets. Keep instruction bodies, clobbers, and interruption state explicit.
- A small physical-entry installation helper for the repeated token-liveness,
  publication, and typed-entry lookup sequence. Prefer a generic Rust helper
  where the type system suffices; extend existing installation machinery before
  adding another macro or cache abstraction.
- A declared instruction-window matcher only for fixed physical contracts that
  remain necessary. Derive length and opcode checks from the same declaration;
  keep operand relations and exceptional guards visible. This reduces LOC but
  does not substitute for generalizing loop selection.

## Test coverage and acceptance criteria

Existing tests already exercise relocation rejection, artifact identity, ABI
contracts, liveness, aliasing, native/fallback transitions, exception boundaries,
cache ownership, and arena lifetime. Extend those facilities rather than building
a parallel harness.

| Area | Next useful coverage |
| --- | --- |
| Numeric/bitwise | NaN, both zeros, infinities, fractional conversion, signed/unsigned boundaries, shift counts outside 0–31; repeated inputs and reversed operand order |
| Control | Both branch arms, joins with live values, interior-entry rejection, and changed facts between invocations |
| Properties/calls | Descriptor/prototype/callee changes, accessors, aliases, receiver lifetime, strictness, exceptions, reentry, and exactly-once observable effects |
| Arrays/loops | Holes, inherited indices, aliased copy backings, empty ranges, nonzero starts, changed bounds, interruption, and every reconstructed live-out |
| Strings | UTF-16 units including lone surrogates, empty strings, changed receivers, and canonical conversion/error behavior |
| Physical targets | Generated objects enabled/disabled, supported host execution, unavailable-artifact fallback, stale token reacquisition, and ABI mismatch rejection |

For each new composition, pair a positive native-entry assertion with a guard
break that proves ordinary fallback. Add metamorphic source variations through
the existing lowering path: renamed locals, moved constants, aliases, and harmless
prologues. Do not reorder operations that can affect JavaScript results or effects.
Use the local Node oracle for externally observable equivalence and relevant
upstream sources whenever Node host/API behavior changes.

Collect diagnostics through the existing execution-profile/trace mechanisms:
miss reasons, actual entry kind, completed operations, guard failures, code bytes,
and retained metadata. Keep these out of production selection. Prioritize work by
production execution frequency and measured cost, never fixture or suite identity.

## Validation during this review

- Extractor and generated artifact integration tests: 19 passed.
- Full `--lib stencil` filter: 270 passed, 6 failed in the reviewed working tree.
- Re-running that filter with both macro changes removed reproduced the same
  270 passes and six failures; these failures predate this refactor.
- Five failures request old fixture names (`add_chain`, `property_own_monomorphic`,
  `property_megamorphic_fallback`, `property_numeric_dot`, and
  `property_numeric_dot_fallback`); the files now carry numeric prefixes.
- The remaining ABI-shape test assumes all `ScalarI32` legacy rows have 5 or 8
  bytes and simple binary/unary-return shapes. The generated-only
  `bitwise_shift_mask_return` row has empty legacy bytes and a longer operation
  sequence. Update coverage to distinguish legacy availability from the selected
  generated artifact, rather than weakening physical validation.

The larger coverage and composition changes above are recommendations; the code
changes in this review are limited to the two mechanical macros.
