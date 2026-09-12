# 228 — Primary-source algorithm research, round nine

Status: complete

Research additional VM/compiler algorithms under the standing constraints: every
function runs stencils on first execution; rustc/LLVM cooks only general templates AOT;
runtime work is copy, patch, and semantic-case repair; no interpreter fallback, hotness
threshold, V8v7-trained PGO, benchmark identity, or exact source spelling selects code.

## New findings

### 1. Compose method lookup and call as one dependency-typed morphism

JavaScriptCore documents the production fast form of `receiver.method(arguments)` as a
receiver structure check followed by a direct call. Structure-transition and
property-replacement watchpoints prove the prototype method remains constant, so there
is no repeated prototype walk, method `Value` materialization, or second callable check.
Its polyvariant devirtualization improved RayTrace by 38%; exposing polymorphic heap
arms to LLVM improved DeltaBlue by 18%. These are upstream results, not predictions.

This sharpens Tasks 155 and 163. The earlier Task 167 stencil failed because it preserved
the Rust call boundary. The next experiment must compose property proof, callee entry,
guest frame, and return continuation into one linked image. The generic miss remains a
shared semantic kernel and performs the effect exactly once.

### 2. Treat speculative failure as expensive; prefer dependencies or IC repair

JavaScriptCore's measured model puts optimizing-tier exit cost orders of magnitude above
an individual successful speculation benefit. For this VM, a guard is acceptable when
it is proven by static context, backed by a one-way dependency fuse, or repairs a local
IC arm. Do not build a large speculative region that routinely reconstructs a Rust
frame on failure. This reinforces Tasks 145, 155, and 164 and makes canonical side-exit
state a prerequisite for aggressive region formation.

A 2025 Hopc study is also useful negative evidence: dynamic binary modification that
merely removed IC memory accesses did not shorten execution on its evaluated modern
hardware. Therefore inline slabs are necessary to erase Rust/helper seams and enable
direct composition, but they are not by themselves a path from the current score to C
performance. Typed regions, calls, ownership, and allocation remain higher leverage.

### 3. Make indexed strings truly indexed

The local implementation uses UTF-8 `chars().nth(index)` for `charAt` and
`charCodeAt`. Real V8v7 loops in Crypto and EarleyBoyer repeatedly index strings, making
those scans quadratic here. V8 uses sequential Latin-1 or UTF-16 code-unit storage for
direct access, with cons/sliced/thin forms only as explicit alternate representations.
Task 227 now owns the concrete O(1) character-access acceptance test; Tasks 11 and 190
own the canonical string representation. Do not add a benchmark-only string shortcut.

### 4. Preserve one guest call frame and actual argument count

V8 removed its arguments-adaptor frame by reversing argument order, keeping arguments in
the caller area, and recording actual count in the callee frame. It reports improvements
of 4.6% in Richards and 6.1% in EarleyBoyer. Tasks 146, 168, 187, and 188 already own the
right destination. Exact-arity entries are an optimization of one canonical guest frame,
not a parallel frame representation; over-application must not allocate an adapter.

### 5. Allocation sinking is a graph transformation

JavaScriptCore's allocation sinking is a must-points-to analysis capable of eliminating
graphs including cycles. Task 176 now requires a quoted virtual-object graph and derived
edge materialization scripts. This is more general than scalar-replacing one literal at
a time and pairs naturally with Task 173's MemorySSA facts and Task 191's dominance
folding.

### 6. Keep static code layout profile-free

LLVM can derive branch probabilities from loop structure and semantic coldness
(`cold`, `unreachable`, `noreturn`, unwind) and propagates block frequency through loop
DAGs. Task 209 should emit these facts and let LLVM own placement/tail duplication.
Task 52 no longer proposes training templates on V8v7; release flags may be A/B tested,
but benchmark execution profiles cannot shape the accepted catalog.

## Priority after this round

1. Finish Tasks 146/181/203: one pointer-bump guest stack and pinned connector ABI.
2. Compose Tasks 155/163/145: watchpoint-backed method lookup directly into callee code.
3. Implement Task 149's finite I32/F64 semantic derivatives and Task 144's static BBV.
4. Land Task 227 through the canonical Tasks 11/190 string representation.
5. Replace Rc ownership with Tasks 148/162/193, then unlock graph allocation sinking
   through Tasks 173/176/191.
6. Apply Task 209 static metadata only where disassembly proves LLVM's existing layout
   is wrong.

This order is intentionally coarse. It attacks call, representation, and ownership
boundaries before adding more leaf stencils; higher-level composites remain ordinary
category morphisms built from the same quoted semantics.

## Primary sources

- JavaScriptCore watchpoints, ICs, speculation economics, and allocation sinking:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>
- JavaScriptCore polyvariant devirtualization and polymorphic inlining:
  <https://webkit.org/blog/3362/introducing-the-webkit-ftl-jit/>
- V8 argument-adaptor-frame removal:
  <https://v8.dev/blog/adaptor-frame>
- V8 string representation:
  <https://chromium.googlesource.com/v8/v8.git/+/refs/heads/main/docs/objects/strings.md>
- Typed shapes plus BBV:
  <https://arxiv.org/abs/1507.02437>
- Hopc inline-cache dynamic-binary-modification negative result:
  <https://arxiv.org/abs/2502.20547>
- LLVM profile-free branch probability and block frequency:
  <https://llvm.org/docs/doxygen/BranchProbabilityInfo_8cpp_source.html> and
  <https://llvm.org/docs/BlockFrequencyTerminology.html>
- Cargo/rustc optimization controls:
  <https://doc.rust-lang.org/cargo/reference/profiles.html> and
  <https://doc.rust-lang.org/rustc/codegen-options/index.html>
- Immix mark-region collector, retained as Task 148's heap basis rather than a duplicate
  collector task: <https://www.steveblackburn.org/pubs/papers/immix-pldi-2008.pdf>
