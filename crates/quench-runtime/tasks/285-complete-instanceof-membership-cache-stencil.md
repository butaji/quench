# 285 — Complete `instanceof` membership-cache condition stencil

Status: complete

Replace repeated prototype-chain walks with one monomorphic, per-bytecode membership
observation and consume that observation in a whole-condition rustc/LLVM-cooked stencil.
The cache key is the constructor `Value` identity plus the receiver's immediate-prototype
storage identity and the VM prototype-mutation epoch; the value is the result of the
complete canonical chain walk. This is the project-local equivalent of V8's historical
`(function, object map) -> answer` `instanceof` cache and its maintained prototype-validity
cell discipline.

The accepted implementation covers both normalized dead-result forms:

- `LoadLocal; LoadName; InstanceOf; JumpIfFalse`; and
- `LoadLocal; LoadName; InstanceOf; Unary(Not); JumpIfFalse`.

Each copied stencil calls one shared immutable Rust kernel that resolves the constructor
through the existing name IC and computes or reuses the complete membership observation.
It then consumes the returned predicate directly as a branch, without materializing the
four or five dead bytecode-register values. The expensive semantics and mutable cache live
outside copied code; only the 108/112-byte control morphisms are instantiated per block.
Selection depends only on exact bytecode dataflow and liveness. It never depends on source
identity, names, benchmark identity, runtime counts, or hotness.

Prototype-chain mutation increments one canonical epoch at every supported mutation edge.
All raw offsets, instruction positions, boolean encodings, and epoch transitions use named
constants plus layout assertions; no representation magic numbers are permitted.

Acceptance: selector and near-miss tests, raw-layout checks, positive/negative/inherited
membership cache tests, constructor and prototype-chain mutation invalidation, full release
tests, residual counters showing the two target families leave the generic executor on
cache hits, and alternating Earley-Boyer plus full V8v7 A/B. Retain only on repeatable gain
without standing component-floor violations.

Primary sources:

- V8's global `instanceof` cache and mutation-clearing invariant:
  <https://chromium.googlesource.com/v8/v8/+/dfaec1393ed70c11e2b326e87cdca5630062abc6/src/ppc/code-stubs-ppc.cc>
- V8 prototype validity-cell invalidation:
  <https://chromium.googlesource.com/v8/v8/+/c4e66b89b4ecd0e90b31e9e4ed08d38085a84c49/src/objects/js-objects.cc>
- JavaScriptCore's `instanceof` bytecode metadata model:
  <https://github.com/WebKit/WebKit/blob/main/Source/JavaScriptCore/bytecode/BytecodeList.rb>

## Result

The first implementation put the complete cache check directly in two 176-byte copied
stencils. It removed almost all exact target residual entries, but ten alternating 500 ms
Earley-Boyer pairs measured 2272.5 to 1853.5 (**-18.44%**) in
`reports/task285-instanceof-earley-ab-10/comparison.txt`. It also enrolled the `LoadName`
sites in the frame snapshot tail. That design was rejected.

The accepted form keeps the complete observation in one shared kernel and the branch in
the copied stencil. `DirectBlockTemplate::InstanceOfCondition` deliberately does not
request a name snapshot; the shared kernel uses the existing `NameIc`, borrows the receiver
local, and clones only the resolved constructor value required by the current ownership
model. The per-bytecode cache records constructor bits, the receiver's immediate-prototype
storage word, the global prototype epoch, and the complete true/false answer. Every
supported prototype mutation advances the named epoch. Focused tests cover exact and
near-miss selection, positive and negative complete membership, and invalidation after
prototype mutation. All **101** release tests pass.

The release catalog contains 127 symbols and 12,172 bytes; the new handlers are 108 and
112 bytes. Residual instrumentation confirms that the exact four- and five-op blocks no
longer enter the generic block executor. Six alternating 500 ms Earley-Boyer pairs measured
2253 to 2287.5 (**+1.53%**) in
`reports/task285-instanceof-shared-kernel-actual-earley-ab-6/comparison.txt`.

The complete five-pair, 300 ms comparison in
`reports/task285-instanceof-full-ab-5/comparison.txt` measured 1906.26 to **1908.14**
(**+0.10%**). Earley-Boyer improved **2.60%**; every suite remained above the standing
-5% component floor. This is a localized accepted coverage gain, not a material movement
toward the 10000 gate. The accepted release binary SHA-256 is
`0be96edbf24bff5f9bc33e7af5a3efde42cd243afd1a9eebb90adb3eb0cf666c`.

`reports/task285-instanceof-shared-kernel-earley-ab-6/comparison.txt` is explicitly invalid:
it accidentally reran the rejected executable because `cargo test --release` refreshed the
test harness but not `target/release/deegen`. All accepted measurements followed an
explicit `cargo build --release`, and the recorded candidate hash was verified before use.
