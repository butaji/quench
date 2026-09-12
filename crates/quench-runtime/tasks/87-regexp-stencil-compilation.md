# 87 — RegExp pattern compiled to a native matcher stencil

Status: planned

The original premise below was disproved by Task 142's native profile: the VM already
uses Rust `regex`, which compiles patterns to optimized automata rather than re-walking
a naive host backtracking interpreter. The actual dominant RegExp cost was recompiling
literal automata on every bytecode execution; [[142-shared-regexp-literal-kernels]]
addresses that first. Keep this task planned only for a later matcher-code experiment
if a post-142 profile shows matching itself dominates and a rustc/LLVM stencil can beat
the existing engine without losing supported semantics.

Compile a regex literal's parsed AST to its own rustc/LLVM stencil the same way a
`DynOp` becomes a native stencil in [[36-direct-opcode-stencils]]: literal characters,
character classes, and anchors become straight-line compares; `*`/`+`/`?` become native
loops; alternation and small bounded groups become native branches. The stencil takes
the subject string pointer/length and start offset and returns a match result (matched
span, or a `Kernel` no-match exit) with no per-character dispatch through a generic
matcher. Patterns using backreferences or lookbehind (rare in the target suite) fall
back to a generic interpreter kernel rather than blocking compilation of the rest of the
pattern's alternatives.

Evidence motivating any future priority must be a post-142 matcher profile, not the
stale generic-backtracking assumption.

Acceptance: a literal-pattern regex used in `test`/`exec`/`replace` compiles to one
linked stencil per distinct pattern (deduplicated by pattern text the way
[[16-kernel-dedup-by-identity]] dedupes kernels); correctness passes the existing RegExp
tests; a pattern requiring backreference/lookbehind correctly falls back without
blocking sibling patterns; alternating A/B on regex-touching suites shows a measured
gain before this becomes the default path.

If matcher-only profiling justifies this task, begin with a bounded quick-check plan
derived from the parsed pattern: preload a small fixed character window, reject
impossible alternatives with word-sized masks, and use a Boyer–Moore-style skip table for
unanchored patterns with a selective literal/class window. This prefilter is part of the
one native matcher plan, not a second wrapper in front of the existing host engine.
Named constants cap lookahead, preload width, recursion, and skip-table size.

Primary implementation precedent: V8's RegExp compiler derives bounded lookahead,
quick-check details, character preloads, and Boyer–Moore skip instructions from the
pattern graph:
<https://chromium.googlesource.com/v8/v8/+/c9b71fac463dacde93fcc5ad77f56ba6ad7eeae6/src/regexp/regexp-compiler.cc>.

Task 253's current native profile now satisfies the “matcher-only evidence” prerequisite:
on the RegExp suite, `regex_automata`'s bounded backtracker is 13.61% of samples and
hybrid forward search is 6.25%, while allocation/free/memmove account for another large
share. Promote the first experiment to a general pattern-IR reducer plus AOT matcher
tiler: fuse load/check/advance loops, coalesce adjacent character-class members into
ranges, compare bounded Latin-1 runs a machine word at a time, and build quick-check or
Boyer-Moore prefilters for unanchored selective literals/classes. V8 reports up to 2x
from threaded/fused regexp bytecodes; JSC documents an eight-character 64-bit compare
and dramatic wins from class coalescing and broader JIT coverage. Those are external
results, not projected local gains. Sources: <https://v8.dev/blog/regexp-tier-up> and
<https://webkit.org/blog/8685/introducing-the-jetstream-2-benchmark-suite/>.

Because this VM forbids hotness tiering, compile the finite matcher plan at RegExp
creation and execute it from the first match. Unsupported backreference/lookaround or
Unicode cases enter a shared semantic kernel. The fallback is another native kernel,
never a JS interpreter or execution-count-triggered tier.

## Round-forty-three plan classification

Before emitting native matcher fragments, classify the quoted pattern as
`MatcherPlan::OnePass | MatcherPlan::WordNfa | MatcherPlan::General`. RE2's one-pass analysis
recognizes patterns where the current input unit uniquely determines the next alternative;
those patterns need neither backtracking nor an NFA thread queue and can lower to a compact
state/character transition loop with capture actions. A small epsilon-closed NFA whose active
set fits `REGEXP_WORD_NFA_STATE_BITS` may use a word-sized bitset transition. Everything else
uses the existing compiled Rust regex kernel. The plan sum, state bound, character width, and
capture obligations are immutable data; no execution counter or benchmark identity selects a
matcher.

Primary source for the one-pass criterion and implementation:
<https://github.com/google/re2/blob/main/re2/onepass.cc>.
